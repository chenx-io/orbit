//! Unified request execution guard: timeout + cancellation.
//!
//! Every protocol client's `execute` is wrapped by [`execute_guarded`], ensuring:
//! - `ProtocolRequest.timeout` actually takes effect (historically no protocol client implemented timeouts);
//! - Load-test cancellation (`CancellationToken`) can interrupt in-flight requests instead of waiting for them to finish naturally.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::traits::ProtocolClient;
use crate::types::{ProtocolError, ProtocolRequest, ProtocolResponse};

/// Default timeout (when the request does not specify one): 30s.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Protocol execution wrapper with timeout and cancellation.
///
/// - The timeout covers the whole connect/send/receive process;
/// - When `cancel` fires it returns `ProtocolError::Protocol("cancelled")` immediately;
/// - The request's own `timeout` takes precedence, otherwise [`DEFAULT_TIMEOUT`] is used.
pub async fn execute_guarded(
    client: &mut dyn ProtocolClient,
    request: ProtocolRequest,
    cancel: &CancellationToken,
) -> Result<ProtocolResponse, ProtocolError> {
    let timeout = request.timeout.unwrap_or(DEFAULT_TIMEOUT);
    tokio::select! {
        r = client.execute(request) => r,
        _ = tokio::time::sleep(timeout) => Err(ProtocolError::Timeout(timeout)),
        _ = cancel.cancelled() => Err(ProtocolError::Protocol("cancelled".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use async_trait::async_trait;

    use crate::traits::ProtocolClient;
    use crate::types::{ProtocolRequest, ProtocolResponse, ProtocolTimings};

    /// Dummy client with a configurable delay: sleeps, then returns success
    struct SlowClient {
        delay: Duration,
    }

    #[async_trait]
    impl ProtocolClient for SlowClient {
        fn name(&self) -> &str {
            "slow"
        }
        fn clone_client(&self) -> Box<dyn ProtocolClient> {
            Box::new(SlowClient { delay: self.delay })
        }
        async fn execute(
            &mut self,
            _request: ProtocolRequest,
        ) -> Result<ProtocolResponse, ProtocolError> {
            tokio::time::sleep(self.delay).await;
            Ok(ProtocolResponse {
                status_code: 200,
                metadata: vec![],
                payload: vec![],
                message_count: 0,
                timings: ProtocolTimings::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_timeout_fires() {
        let mut client = SlowClient {
            delay: Duration::from_secs(5),
        };
        let cancel = CancellationToken::new();
        let request = ProtocolRequest {
            target: "http://localhost".into(),
            operation: "GET".into(),
            timeout: Some(Duration::from_millis(50)),
            ..Default::default()
        };
        let result = execute_guarded(&mut client, request, &cancel).await;
        match result {
            Err(ProtocolError::Timeout(_)) => {}
            other => panic!("expected Timeout, got {:?}", other.map(|_| ())),
        }
    }

    #[tokio::test]
    async fn test_cancel_interrupts_inflight() {
        let mut client = SlowClient {
            delay: Duration::from_secs(5),
        };
        let cancel = CancellationToken::new();
        let cancel_task = cancel.clone();
        let request = ProtocolRequest {
            target: "http://localhost".into(),
            operation: "GET".into(),
            timeout: Some(Duration::from_secs(30)),
            ..Default::default()
        };
        let run = execute_guarded(&mut client, request, &cancel);
        tokio::pin!(run);

        // Cancel after 50ms; it should return cancelled immediately instead of waiting 5s
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel_task.cancel();
        let result = run.await;
        match result {
            Err(ProtocolError::Protocol(msg)) if msg == "cancelled" => {}
            other => panic!("expected cancelled, got {:?}", other.map(|_| ())),
        }
    }

    #[tokio::test]
    async fn test_fast_request_passes() {
        let mut client = SlowClient {
            delay: Duration::from_millis(10),
        };
        let cancel = CancellationToken::new();
        let request = ProtocolRequest {
            target: "http://localhost".into(),
            operation: "GET".into(),
            timeout: Some(Duration::from_secs(5)),
            ..Default::default()
        };
        let result = execute_guarded(&mut client, request, &cancel).await;
        assert!(result.is_ok());
    }
}
