//! Native TOML/YAML parsing and serialization for trusted scripts.
//! Exposes a JSON-compatible subset: only string-keyed maps and serializable scalars.

use serde::Deserialize;
use serde_json::{Value, json};

const MAX_TEXT: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    operation: String,
    text: Option<String>,
    value: Option<Value>,
}

fn run(input: Request) -> Result<Value, String> {
    match input.operation.as_str() {
        "toml_decode" => {
            let source = input.text.ok_or("TOML decode needs text")?;
            if source.len() > MAX_TEXT {
                return Err("TOML document exceeds 4 MiB".into());
            }
            toml::from_str::<Value>(&source).map_err(|err| err.to_string())
        }
        "yaml_decode" => {
            let source = input.text.ok_or("YAML decode needs text")?;
            if source.len() > MAX_TEXT {
                return Err("YAML document exceeds 4 MiB".into());
            }
            yaml_serde::from_str::<Value>(&source).map_err(|err| err.to_string())
        }
        "toml_encode" => {
            let value = input.value.ok_or("TOML encode needs a table")?;
            let output = toml::to_string(&value).map_err(|err| err.to_string())?;
            if output.len() > MAX_TEXT {
                return Err("TOML output exceeds 4 MiB".into());
            }
            Ok(json!(output))
        }
        "yaml_encode" => {
            let value = input.value.ok_or("YAML encode needs a value")?;
            let output = yaml_serde::to_string(&value).map_err(|err| err.to_string())?;
            if output.len() > MAX_TEXT {
                return Err("YAML output exceeds 4 MiB".into());
            }
            Ok(json!(output))
        }
        _ => Err("unrecognized data operation".into()),
    }
}

/// JSON-native host bridge for TOML and YAML.
pub fn serialized(input: String) -> String {
    let result = serde_json::from_str::<Request>(&input)
        .map_err(|err| format!("invalid data request: {err}"))
        .and_then(run);
    match result {
        Ok(value) => json!({"value":value,"error":null}).to_string(),
        Err(err) => json!({"value":null,"error":err}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_toml_does_not_panic() {
        let invalid = serialized(r#"{"operation":"toml_decode","text":"name = ["}"#.into());
        assert!(invalid.contains("error"));
    }

    #[test]
    fn rejects_unsupported_operation() {
        assert!(serialized(r#"{"operation":"unknown"}"#.into()).contains("unrecognized"));
    }
}
