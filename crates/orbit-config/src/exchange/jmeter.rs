//! JMeter JMX importer (XML test plan -> ApiSpec + sequential executor)
//!
//! Coverage (baseline: jmeter.apache.org/usermanual):
//! - HTTPSamplerProxy（domain/port/path/method/protocol/body）
//! - HeaderManager / Arguments
//! - ThreadGroup (num_threads/ramp_time/loops -> executor hint)
//! - ResponseAssertion（response_code Equals → Status；response_data Contains → BodyContains）
//! - ConstantTimer (-> pre-request wait of the next sampler)

use super::ir::{ApiSpec, EndpointSpec};
use super::{ImportError, ImportFormat};
use crate::model::check::{Check, CheckKind};
use crate::model::plan::Executor;
use crate::model::request::{HttpRequestConfig, RequestSpec};

/// JMeter: import only (JMX test plan restored as a sequential-execution scenario)
pub(crate) struct JmeterImporter;

impl ImportFormat for JmeterImporter {
    fn name(&self) -> &'static str {
        "jmeter"
    }

    fn parse(&self, xml: &str) -> Result<ApiSpec, ImportError> {
        // collect events (sampler / timer / assertion) by document position, preserving ordering semantics
        let mut events: Vec<(usize, JmxEvent)> = Vec::new();

        let sampler_re = regex::Regex::new(r"<HTTPSamplerProxy[^>]*>[\s\S]*?</HTTPSamplerProxy>")
            .map_err(|e| ImportError::Parse(e.to_string()))?;
        for cap in sampler_re.captures_iter(xml) {
            let block = cap.get(0).unwrap();
            if let Some((name, req)) = parse_jmeter_sampler(block.as_str()) {
                events.push((block.start(), JmxEvent::Sampler(name, req)));
            }
        }

        let timer_re = regex::Regex::new(r"<ConstantTimer[^>]*>[\s\S]*?</ConstantTimer>")
            .map_err(|e| ImportError::Parse(e.to_string()))?;
        for cap in timer_re.captures_iter(xml) {
            let block = cap.get(0).unwrap();
            if let Some(ms) = parse_constant_timer(block.as_str()) {
                events.push((block.start(), JmxEvent::Timer(ms)));
            }
        }

        let assert_re = regex::Regex::new(r"<ResponseAssertion[^>]*>[\s\S]*?</ResponseAssertion>")
            .map_err(|e| ImportError::Parse(e.to_string()))?;
        for cap in assert_re.captures_iter(xml) {
            let block = cap.get(0).unwrap();
            if let Some(check) = parse_response_assertion(block.as_str()) {
                events.push((block.start(), JmxEvent::Assertion(check)));
            }
        }

        events.sort_by_key(|e| e.0);

        let mut spec = ApiSpec::new("JMeter import");
        let mut pending_wait: u64 = 0;
        for (_, ev) in events {
            match ev {
                JmxEvent::Timer(ms) => pending_wait = ms,
                JmxEvent::Assertion(check) => {
                    if let Some(ep) = spec.endpoints.last_mut() {
                        ep.checks.push(check);
                    }
                }
                JmxEvent::Sampler(name, req) => {
                    let mut ep = EndpointSpec::new(name, RequestSpec::Http(Box::new(req)));
                    ep.wait_ms = Some(pending_wait);
                    pending_wait = 0;
                    spec.endpoints.push(ep);
                }
            }
        }

        if spec.endpoints.is_empty() {
            return Err(ImportError::Parse("No HTTP Samplers found in JMX".into()));
        }

        // ThreadGroup -> executor hint (loop count = iterations; threads/ramp-up pass through to extensions)
        let (threads, ramp, loops) = parse_thread_group(xml);
        spec.executor = Some(Executor::Sequential { iterations: loops });
        spec.extensions
            .insert("jmeter_threads".into(), serde_json::json!(threads));
        spec.extensions
            .insert("jmeter_ramp_secs".into(), serde_json::json!(ramp));
        Ok(spec)
    }
}

#[allow(clippy::large_enum_variant)]
enum JmxEvent {
    Sampler(String, HttpRequestConfig),
    Timer(u64),
    Assertion(Check),
}

/// Parse a single `<HTTPSamplerProxy>` block, returning (name, HttpRequestConfig)
fn parse_jmeter_sampler(block: &str) -> Option<(String, HttpRequestConfig)> {
    let name = regex::Regex::new(r#"testname="([^"]*)""#)
        .ok()?
        .captures(block)?
        .get(1)?
        .as_str()
        .to_string();

    let prop = |n: &str| -> Option<String> {
        let re_str = &format!(r#"<stringProp name="{}">([^<]*)</stringProp>"#, n);
        regex::Regex::new(re_str)
            .ok()?
            .captures(block)?
            .get(1)
            .map(|m| m.as_str().to_string())
    };

    let domain = prop("HTTPSampler.domain").unwrap_or_default();
    let port = prop("HTTPSampler.port").unwrap_or_else(|| "80".into());
    let path = prop("HTTPSampler.path").unwrap_or_else(|| "/".into());
    let method = prop("HTTPSampler.method").unwrap_or_else(|| "GET".into());
    let protocol = prop("HTTPSampler.protocol").unwrap_or_else(|| "https".into());

    let url = if domain.is_empty() {
        return None;
    } else {
        let base = format!("{}://{}", protocol, domain.trim());
        if domain.contains(":") || port == "80" || port == "443" || port.is_empty() {
            format!("{}{}", base, path)
        } else {
            format!("{}:{}{}", base, port, path)
        }
    };

    // Headers from HeaderManager
    let mut headers = std::collections::HashMap::new();
    let hdr_re = regex::Regex::new(r#"<stringProp name="Header.name">([^<]*)</stringProp>\s*<stringProp name="Header.value">([^<]*)</stringProp>"#).ok();
    if let Some(re) = &hdr_re {
        for cap in re.captures_iter(block) {
            headers.insert(cap[1].to_string(), cap[2].to_string());
        }
    }

    // Body from Arguments
    let body = regex::Regex::new(r#"<stringProp name="Argument.value">([^<]*)</stringProp>"#)
        .ok()
        .and_then(|re| re.captures(block))
        .and_then(|c| c.get(1))
        .map(|m| {
            let s = m.as_str().to_string();
            serde_json::from_str::<serde_yaml::Value>(&s).unwrap_or(serde_yaml::Value::String(s))
        });

    Some((
        name,
        HttpRequestConfig {
            method: method.to_uppercase(),
            url,
            headers,
            body,
            timeout: "30s".into(),
            payload_format: None,
            grpc_service: None,
            grpc_use_reflection: false,
            response_format: None,
        },
    ))
}

/// ConstantTimer -> delay in milliseconds.
fn parse_constant_timer(block: &str) -> Option<u64> {
    let re =
        regex::Regex::new(r#"<stringProp name="ConstantTimer.delay">([^<]*)</stringProp>"#).ok()?;
    re.captures(block)?.get(1)?.as_str().parse().ok()
}

/// ResponseAssertion -> scenario Check.
/// Supports: response_code + Equals/Contains -> Status; response_data + Contains -> BodyContains.
fn parse_response_assertion(block: &str) -> Option<Check> {
    let prop = |n: &str| -> Option<String> {
        let re =
            regex::Regex::new(&format!(r#"<stringProp name="{}">([^<]*)</stringProp>"#, n)).ok()?;
        re.captures(block)?.get(1).map(|m| m.as_str().to_string())
    };
    let test_field = prop("Assertion.test_field")?;
    let test_type = prop("Assertion.test_type")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);
    // assertion comparison string: the stringProp value under <collectionProp name="Asserion.test_string">
    let value = regex::Regex::new(r#"<collectionProp name="Asserion.test_string">[\s\S]*?<stringProp name="\d+">([^<]*)</stringProp>"#)
        .ok()?
        .captures(block)?
        .get(1)?
        .as_str()
        .to_string();

    match test_field.as_str() {
        // 1=Contains, 2=Equals, 4=Matches
        "Assertion.response_code" if matches!(test_type, 1 | 2 | 4) => {
            value.parse::<i32>().ok().map(|v| Check {
                kind: CheckKind::Status { value: v },
                meta: None,
            })
        }
        // 1=Contains, 3=Substring -> BodyContains; 2=Equals -> anchored regex
        "Assertion.response_data" if matches!(test_type, 1..=3) => {
            if test_type == 2 {
                Some(Check {
                    kind: CheckKind::Regex {
                        pattern: format!("^{}$", regex::escape(&value)),
                    },
                    meta: None,
                })
            } else {
                Some(Check {
                    kind: CheckKind::BodyContains { value },
                    meta: None,
                })
            }
        }
        _ => None,
    }
}

/// Parse ThreadGroup: returns (num_threads, ramp_time, loops). Defaults to (1, 0, 1).
fn parse_thread_group(xml: &str) -> (u32, u64, u32) {
    let prop = |name: &str| -> Option<String> {
        let re = regex::Regex::new(&format!(
            r#"<stringProp name="{}">([^<]*)</stringProp>"#,
            name
        ))
        .ok()?;
        re.captures(xml)?.get(1).map(|m| m.as_str().to_string())
    };
    let threads = prop("ThreadGroup.num_threads")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let ramp = prop("ThreadGroup.ramp_time")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let loops = prop("LoopController.loops")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(1)
        .max(1);
    (threads, ramp, loops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jmeter_import() {
        let jmx = r#"<?xml version="1.0"?>
<jmeterTestPlan>
  <hashTree>
    <ThreadGroup>
      <stringProp name="ThreadGroup.num_threads">5</stringProp>
      <stringProp name="ThreadGroup.ramp_time">10</stringProp>
      <stringProp name="LoopController.loops">3</stringProp>
    </ThreadGroup>
    <hashTree>
      <ConstantTimer testname="Think">
        <stringProp name="ConstantTimer.delay">1000</stringProp>
      </ConstantTimer>
      <HTTPSamplerProxy testname="Get Users" enabled="true">
        <stringProp name="HTTPSampler.domain">api.example.com</stringProp>
        <stringProp name="HTTPSampler.port">443</stringProp>
        <stringProp name="HTTPSampler.protocol">https</stringProp>
        <stringProp name="HTTPSampler.path">/api/users</stringProp>
        <stringProp name="HTTPSampler.method">GET</stringProp>
      </HTTPSamplerProxy>
      <ResponseAssertion testname="Status 200">
        <collectionProp name="Asserion.test_string">
          <stringProp name="49586">200</stringProp>
        </collectionProp>
        <stringProp name="Assertion.test_field">Assertion.response_code</stringProp>
        <stringProp name="Assertion.test_type">2</stringProp>
      </ResponseAssertion>
      <hashTree/>
      <HTTPSamplerProxy testname="Create User" enabled="true">
        <stringProp name="HTTPSampler.domain">api.example.com</stringProp>
        <stringProp name="HTTPSampler.port">443</stringProp>
        <stringProp name="HTTPSampler.path">/api/users</stringProp>
        <stringProp name="HTTPSampler.method">POST</stringProp>
      </HTTPSamplerProxy>
      <ResponseAssertion testname="Body contains">
        <collectionProp name="Asserion.test_string">
          <stringProp name="49587">ok</stringProp>
        </collectionProp>
        <stringProp name="Assertion.test_field">Assertion.response_data</stringProp>
        <stringProp name="Assertion.test_type">1</stringProp>
      </ResponseAssertion>
      <hashTree/>
    </hashTree>
  </hashTree>
</jmeterTestPlan>"#;
        let spec = JmeterImporter.parse(jmx).unwrap();
        assert_eq!(spec.endpoints.len(), 2);
        // ThreadGroup: loops -> iterations, threads/ramp-up pass through
        assert!(matches!(
            spec.executor,
            Some(Executor::Sequential { iterations: 3 })
        ));
        assert_eq!(spec.extensions["jmeter_threads"], 5);
        assert_eq!(spec.extensions["jmeter_ramp_secs"], 10);
        // Timer -> pre-request wait of the first sampler; the second has no timer
        assert_eq!(spec.endpoints[0].wait_ms, Some(1000));
        assert_eq!(spec.endpoints[1].wait_ms, Some(0));
        // assertions: status 200 -> Status check; body contains -> BodyContains
        assert_eq!(spec.endpoints[0].checks.len(), 1);
        match &spec.endpoints[0].checks[0].kind {
            CheckKind::Status { value } => assert_eq!(*value, 200),
            _ => panic!("expected Status check"),
        }
        assert_eq!(spec.endpoints[1].checks.len(), 1);
        match &spec.endpoints[1].checks[0].kind {
            CheckKind::BodyContains { value } => assert_eq!(value, "ok"),
            _ => panic!("expected BodyContains check"),
        }
    }
}
