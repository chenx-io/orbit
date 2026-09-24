//! Agent registry: uniformly manages state, heartbeats, resources and task command channels for both agent modes.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

use crate::proto::TaskCommand;
use crate::types::{
    AgentInfo, AgentMetricsSnapshot, AgentState, DistributedError, ResourceSnapshot,
};

/// Number of resource history points retained (trend chart)
const RESOURCE_HISTORY_LEN: usize = 60;

/// Registry event (pushed to the frontend / Tauri)
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RegistryEvent {
    AgentUpdated {
        agent: AgentInfo,
    },
    AgentRemoved {
        agent_id: String,
    },
    /// Task event (load-test progress/result, forwarded to the frontend by handle_task_event)
    TaskEvent {
        agent_id: String,
        task_id: String,
        /// started / progress / finished / failed
        kind: String,
        state: AgentState,
        metrics: Option<AgentMetricsSnapshot>,
    },
}

/// Registration record for a single agent
pub struct AgentRecord {
    pub info: AgentInfo,
    /// controller → agent task command channel (unified for both modes)
    pub cmd_tx: mpsc::Sender<TaskCommand>,
    resource_history: VecDeque<ResourceSnapshot>,
}

#[derive(Clone)]
pub struct AgentRegistry {
    inner: Arc<Mutex<HashMap<String, AgentRecord>>>,
    events: broadcast::Sender<RegistryEvent>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RegistryEvent> {
        self.events.subscribe()
    }

    /// Register an agent (single entry point for both modes). An existing, non-offline agent rejects duplicate registration.
    #[allow(clippy::too_many_arguments)]
    pub fn register(
        &self,
        id: String,
        mode: String,
        addr: String,
        version: String,
        cpu_cores: u32,
        memory_mb: u64,
        labels: Vec<(String, String)>,
        cmd_tx: mpsc::Sender<TaskCommand>,
    ) -> Result<(), DistributedError> {
        let mut m = self.inner.lock().unwrap();
        if let Some(rec) = m.get(&id) {
            if rec.info.state != AgentState::Offline {
                return Err(DistributedError::AlreadyRegistered(id));
            }
        }
        let now = now_ms();
        let info = AgentInfo {
            id,
            mode,
            addr,
            version,
            cpu_cores,
            memory_mb,
            labels,
            state: AgentState::Idle,
            degraded: false,
            taken_over: false,
            last_heartbeat_ms: now,
            registered_at_ms: now,
            task_id: None,
            resource: None,
            resource_history: Vec::new(),
        };
        m.insert(
            info.id.clone(),
            AgentRecord {
                info: info.clone(),
                cmd_tx,
                resource_history: VecDeque::new(),
            },
        );
        drop(m);
        self.emit(RegistryEvent::AgentUpdated { agent: info });
        Ok(())
    }

    /// Re-register (reconnect after going offline): restore to Idle
    pub fn reconnect(&self, id: &str) {
        self.set_state(id, AgentState::Idle);
    }

    pub fn unregister(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
        self.emit(RegistryEvent::AgentRemoved {
            agent_id: id.to_string(),
        });
    }

    pub fn set_state(&self, id: &str, state: AgentState) {
        let mut m = self.inner.lock().unwrap();
        if let Some(rec) = m.get_mut(id) {
            rec.info.state = state;
            let agent = rec.info.clone();
            drop(m);
            self.emit(RegistryEvent::AgentUpdated { agent });
        }
    }

    pub fn set_task(&self, id: &str, task_id: Option<String>) {
        let mut m = self.inner.lock().unwrap();
        if let Some(rec) = m.get_mut(id) {
            rec.info.task_id = task_id;
            let agent = rec.info.clone();
            drop(m);
            self.emit(RegistryEvent::AgentUpdated { agent });
        }
    }

    pub fn update_heartbeat(&self, id: &str, ts: i64) {
        let mut m = self.inner.lock().unwrap();
        if let Some(rec) = m.get_mut(id) {
            rec.info.last_heartbeat_ms = ts;
            // A heartbeat means the connection is usable; recover from a mistaken offline mark (back to idle only when no task is running)
            if rec.info.state == AgentState::Offline {
                rec.info.state = AgentState::Idle;
            }
            let agent = rec.info.clone();
            drop(m);
            self.emit(RegistryEvent::AgentUpdated { agent });
        }
    }

    pub fn update_resource(&self, id: &str, res: ResourceSnapshot) {
        let mut m = self.inner.lock().unwrap();
        if let Some(rec) = m.get_mut(id) {
            rec.info.resource = Some(res.clone());
            rec.resource_history.push_back(res);
            while rec.resource_history.len() > RESOURCE_HISTORY_LEN {
                rec.resource_history.pop_front();
            }
            rec.info.resource_history = rec.resource_history.iter().cloned().collect();
            let agent = rec.info.clone();
            drop(m);
            self.emit(RegistryEvent::AgentUpdated { agent });
        }
    }

    pub fn set_degraded(&self, id: &str, degraded: bool) {
        let mut m = self.inner.lock().unwrap();
        if let Some(rec) = m.get_mut(id) {
            rec.info.degraded = degraded;
            let agent = rec.info.clone();
            drop(m);
            self.emit(RegistryEvent::AgentUpdated { agent });
        }
    }

    pub fn set_taken_over(&self, id: &str, taken_over: bool) {
        let mut m = self.inner.lock().unwrap();
        if let Some(rec) = m.get_mut(id) {
            rec.info.taken_over = taken_over;
            let agent = rec.info.clone();
            drop(m);
            self.emit(RegistryEvent::AgentUpdated { agent });
        }
    }

    /// Clear all agents (when the controller shuts down)
    pub fn clear(&self) {
        let ids: Vec<String> = self.inner.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.unregister(&id);
        }
    }

    pub fn get(&self, id: &str) -> Option<AgentInfo> {
        self.inner.lock().unwrap().get(id).map(|r| r.info.clone())
    }

    pub fn list(&self) -> Vec<AgentInfo> {
        let m = self.inner.lock().unwrap();
        let mut v: Vec<AgentInfo> = m.values().map(|r| r.info.clone()).collect();
        v.sort_by_key(|a| a.registered_at_ms);
        v
    }

    pub fn cmd_tx(&self, id: &str) -> Option<mpsc::Sender<TaskCommand>> {
        self.inner.lock().unwrap().get(id).map(|r| r.cmd_tx.clone())
    }

    /// Agents eligible for task dispatch: not paused, not offline, no task in progress (in-progress agents are left to the caller's policy)
    pub fn dispatchable(&self) -> Vec<(AgentInfo, mpsc::Sender<TaskCommand>)> {
        let m = self.inner.lock().unwrap();
        m.values()
            .filter(|r| {
                r.info.state != AgentState::Paused
                    && r.info.state != AgentState::Offline
                    && !r.info.degraded
            })
            .map(|r| (r.info.clone(), r.cmd_tx.clone()))
            .collect()
    }

    /// Heartbeat timeout sweep: no heartbeat for longer than timeout → mark OFFLINE (record retained)
    pub fn sweep_offline(&self, timeout_ms: i64) -> Vec<String> {
        let now = now_ms();
        let mut changed = Vec::new();
        let mut m = self.inner.lock().unwrap();
        for rec in m.values_mut() {
            if rec.info.state != AgentState::Offline
                && now - rec.info.last_heartbeat_ms > timeout_ms
            {
                rec.info.state = AgentState::Offline;
                changed.push(rec.info.id.clone());
            }
        }
        drop(m);
        for id in &changed {
            if let Some(a) = self.get(id) {
                self.emit(RegistryEvent::AgentUpdated { agent: a });
            }
        }
        changed
    }

    fn emit(&self, ev: RegistryEvent) {
        let _ = self.events.send(ev);
    }

    /// Forward task events (load-test progress/result) to the frontend
    pub fn emit_task(
        &self,
        agent_id: &str,
        task_id: &str,
        kind: &str,
        state: AgentState,
        metrics: Option<AgentMetricsSnapshot>,
    ) {
        self.emit(RegistryEvent::TaskEvent {
            agent_id: agent_id.to_string(),
            task_id: task_id.to_string(),
            kind: kind.to_string(),
            state,
            metrics,
        });
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
