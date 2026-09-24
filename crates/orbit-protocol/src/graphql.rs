//! GraphQL protocol client
//!
//! Reuses the shared HTTP transport stack (`HttpClient`): automatically builds the GraphQL request body
//! `{ query, variables, operationName }` and POSTs it.

use async_trait::async_trait;
use std::time::Instant;

use crate::http::HttpClient;
use crate::traits::ProtocolClient;
use crate::types::{ProtocolError, ProtocolRequest, ProtocolResponse, ProtocolTimings};

/// GraphQL protocol client (reuses the HTTP transport stack)
pub struct GraphqlClient {
    http: HttpClient,
}

impl GraphqlClient {
    pub fn new() -> Self {
        Self {
            http: HttpClient::new(),
        }
    }
}

impl Default for GraphqlClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProtocolClient for GraphqlClient {
    fn name(&self) -> &str {
        "graphql"
    }
    fn description(&self) -> &str {
        "GraphQL client (query + mutation)"
    }

    async fn execute(
        &mut self,
        request: ProtocolRequest,
    ) -> Result<ProtocolResponse, ProtocolError> {
        let total_start = Instant::now();

        // GraphQL request body: { "query": "...", "variables": {...}, "operationName": "..." }
        let query = String::from_utf8_lossy(&request.payload).to_string();

        let mut variables = serde_json::Value::Null;
        let mut operation_name: Option<String> = None;
        let mut headers: Vec<(String, String)> = Vec::new();

        // Extract variables / operationName from metadata; pass the rest through as request headers
        for (key, value) in &request.metadata {
            match key.as_str() {
                "graphql-variables" => {
                    variables = serde_json::from_str(value).unwrap_or(serde_json::Value::Null);
                }
                "graphql-operation-name" => {
                    operation_name = Some(value.clone());
                }
                _ => headers.push((key.clone(), value.clone())),
            }
        }

        let mut body = serde_json::json!({ "query": query });
        if !variables.is_null() {
            body["variables"] = variables;
        }
        if let Some(name) = &operation_name {
            body["operationName"] = serde_json::Value::String(name.clone());
        }

        let payload = serde_json::to_vec(&body)
            .map_err(|e| ProtocolError::Protocol(format!("JSON encode: {}", e)))?;

        let mut req = request;
        req.operation = "POST".to_string();
        req.payload = payload;
        headers.push(("Content-Type".into(), "application/json".into()));
        req.metadata = headers;

        let resp = self.http.execute(req).await?;
        let duration_ms = total_start.elapsed();
        let _ = duration_ms;

        // Validate the GraphQL response format
        let response_json: serde_json::Value = serde_json::from_slice(&resp.payload)
            .unwrap_or(serde_json::json!({"raw": "<parse error>"}));

        // GraphQL error detection
        let has_errors = response_json.get("errors").is_some();
        let error_count = response_json
            .get("errors")
            .and_then(|e| e.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let mut meta = vec![
            ("protocol".into(), "graphql".into()),
            ("has_errors".into(), has_errors.to_string()),
            ("error_count".into(), error_count.to_string()),
        ];
        meta.extend(resp.metadata);

        Ok(ProtocolResponse {
            status_code: resp.status_code,
            metadata: meta,
            payload: resp.payload,
            message_count: 0,
            timings: ProtocolTimings {
                dns: resp.timings.dns,
                tcp: resp.timings.tcp,
                tls: resp.timings.tls,
                send: resp.timings.send,
                first_byte: resp.timings.first_byte,
                receive: resp.timings.receive,
                total: resp.timings.total,
            },
        })
    }

    fn clone_client(&self) -> Box<dyn ProtocolClient> {
        Box::new(Self::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_graphql_query() {
        let mut client = GraphqlClient::new();
        let query = r#"{ __schema { queryType { name } } }"#;
        let request = ProtocolRequest {
            target: "https://countries.trevorblades.com/graphql".into(),
            operation: "query".into(),
            metadata: vec![],
            payload: query.as_bytes().to_vec(),
            timeout: Some(Duration::from_secs(15)),
            streaming_mode: None,
            payload_format: None,
            response_format: None,
            options: Default::default(),
            connection: None,
        };

        if let Ok(resp) = client.execute(request).await {
            assert!(resp.status_code == 200 || resp.status_code == 503);
            if resp.status_code == 200 {
                let json: serde_json::Value = serde_json::from_slice(&resp.payload).unwrap();
                assert!(json.get("data").is_some() || json.get("errors").is_some());
            }
        }
    }

    #[tokio::test]
    async fn test_graphql_with_variables() {
        let mut client = GraphqlClient::new();
        let query = r#"query($code: ID!) { country(code: $code) { name capital } }"#;
        let request = ProtocolRequest {
            target: "https://countries.trevorblades.com/graphql".into(),
            operation: "query".into(),
            metadata: vec![("graphql-variables".into(), r#"{"code":"CN"}"#.into())],
            payload: query.as_bytes().to_vec(),
            timeout: Some(Duration::from_secs(15)),
            streaming_mode: None,
            payload_format: None,
            response_format: None,
            options: Default::default(),
            connection: None,
        };

        if let Ok(resp) = client.execute(request).await {
            if resp.status_code == 200 {
                let json: serde_json::Value = serde_json::from_slice(&resp.payload).unwrap();
                if let Some(data) = json.get("data") {
                    assert!(data.get("country").is_some());
                }
            }
        }
    }
}
