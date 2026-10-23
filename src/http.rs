//! Trusted-mode HTTP client. Blocking Rust request with a bounded body.
//! No network APIs are installed into sandboxed Lua VMs.

use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;
use ureq::{Agent, Body};

const MAX_BODY: u64 = 4 * 1024 * 1024;
const MAX_TIMEOUT_MS: u64 = 120_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    method: String,
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<String>,
    timeout_ms: Option<u64>,
}

fn handle_response(mut response: ureq::http::Response<Body>) -> Result<Value, String> {
    let status = response.status().as_u16();
    let mut headers = BTreeMap::new();
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            headers.insert(name.to_string(), value.to_owned());
        }
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("failed to receive response body: {err}"))?;
    if bytes.len() as u64 > MAX_BODY {
        return Err("HTTP response exceeds 4 MiB limit".into());
    }
    let body = String::from_utf8(bytes).map_err(|_| "HTTP body must be UTF-8 text".to_string())?;
    Ok(json!({
        "ok": (200..300).contains(&status),
        "status": status,
        "headers": headers,
        "body": body,
    }))
}

fn perform(req: Request) -> Result<Value, String> {
    if !(req.url.starts_with("https://") || req.url.starts_with("http://")) {
        return Err("HTTP URL must start with http:// or https://".into());
    }
    let timeout_ms = req.timeout_ms.unwrap_or(30_000);
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
        return Err(format!("timeout_ms must be between 1 and {MAX_TIMEOUT_MS}"));
    }
    if req.method.is_empty() {
        return Err("HTTP method must not be empty".into());
    }
    let agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(timeout_ms)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let response = match req.method.as_str() {
        "GET" => {
            if req.body.is_some() {
                return Err("GET does not accept a request body".into());
            }
            let mut request = agent.get(&req.url);
            for (name, value) in &req.headers {
                request = request.header(name.as_str(), value.as_str());
            }
            request.call()
        }
        "DELETE" => {
            if req.body.is_some() {
                return Err("DELETE does not accept a request body".into());
            }
            let mut request = agent.delete(&req.url);
            for (name, value) in &req.headers {
                request = request.header(name.as_str(), value.as_str());
            }
            request.call()
        }
        "POST" => {
            let mut request = agent.post(&req.url);
            for (name, value) in &req.headers {
                request = request.header(name.as_str(), value.as_str());
            }
            request.send(req.body.unwrap_or_default())
        }
        "PUT" => {
            let mut request = agent.put(&req.url);
            for (name, value) in &req.headers {
                request = request.header(name.as_str(), value.as_str());
            }
            request.send(req.body.unwrap_or_default())
        }
        "PATCH" => {
            let mut request = agent.patch(&req.url);
            for (name, value) in &req.headers {
                request = request.header(name.as_str(), value.as_str());
            }
            request.send(req.body.unwrap_or_default())
        }
        _ => return Err("allowed methods: GET, POST, PUT, PATCH, DELETE".into()),
    }
    .map_err(|err| format!("HTTP request failed: {err}"))?;
    handle_response(response)
}

/// A host callback consuming and returning JSON, invoked from the Lua adapter.
pub fn serialized(input: String) -> String {
    let result = serde_json::from_str::<Request>(&input)
        .map_err(|err| format!("invalid HTTP request: {err}"))
        .and_then(perform);
    match result {
        Ok(value) => json!({"result":value,"error":null}).to_string(),
        Err(err) => json!({"result":null,"error":err}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_method_and_scheme_without_network() {
        let invalid = serialized(r#"{"method":"GET","url":"file:///etc/passwd"}"#.into());
        assert!(invalid.contains("HTTP URL"));
        let invalid = serialized(r#"{"method":"TRACE","url":"https://example.com"}"#.into());
        assert!(invalid.contains("allowed methods"));
    }
}
