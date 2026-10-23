//! Trusted-mode filesystem helpers exposed to Lua as a small JSON-native bridge.
//! No filesystem capabilities are registered in restricted-mode VMs.

use serde::Deserialize;
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MAX_DEPTH: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    op: String,
    path: String,
    data: Option<String>,
    destination: Option<String>,
}

fn record(path: &Path, kind: &str) -> Value {
    json!({
        "name": path.file_name().unwrap_or_default().to_string_lossy(),
        "path": path.to_string_lossy(),
        "type": kind,
    })
}

fn kind(path: &Path) -> Result<&'static str, String> {
    let meta = fs::symlink_metadata(path).map_err(|err| err.to_string())?;
    let ty = meta.file_type();
    Ok(if ty.is_symlink() {
        "symlink"
    } else if ty.is_dir() {
        "directory"
    } else if ty.is_file() {
        "file"
    } else {
        "other"
    })
}

fn entries(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut values = Vec::new();
    for result in fs::read_dir(path).map_err(|err| err.to_string())? {
        let entry = result.map_err(|err| err.to_string())?;
        values.push(entry.path());
        if values.len() > MAX_ENTRIES {
            return Err("directory entry limit exceeded".into());
        }
    }
    values.sort();
    Ok(values)
}

fn walk(path: &Path, depth: usize, result: &mut Vec<Value>) -> Result<(), String> {
    if depth >= MAX_DEPTH {
        return Err("directory recursion depth exceeded".into());
    }
    for child in entries(path)? {
        if result.len() >= MAX_ENTRIES {
            return Err("directory tree entry limit exceeded".into());
        }
        let file_kind = kind(&child)?;
        result.push(record(&child, file_kind));
        // Deliberately never follow symlinks, to avoid traversal cycles.
        if file_kind == "directory" {
            walk(&child, depth + 1, result)?;
        }
    }
    Ok(())
}

fn perform(request: Request) -> Result<Value, String> {
    if request.path.is_empty() {
        return Err("filesystem path must not be empty".into());
    }
    let path = Path::new(&request.path);
    match request.op.as_str() {
        "exists" => Ok(json!(path.exists())),
        "read" => {
            let file = File::open(path).map_err(|err| err.to_string())?;
            let mut contents = Vec::new();
            file.take(MAX_READ_BYTES + 1)
                .read_to_end(&mut contents)
                .map_err(|err| err.to_string())?;
            if contents.len() as u64 > MAX_READ_BYTES {
                return Err("file is too large for fs.read".into());
            }
            String::from_utf8(contents)
                .map(|s| json!(s))
                .map_err(|_| "fs.read expects UTF-8 text".into())
        }
        "write" => {
            let data = request.data.ok_or("fs.write expects a text string")?;
            if data.len() as u64 > MAX_READ_BYTES {
                return Err("file is too large for fs.write".into());
            }
            fs::write(path, data.as_bytes()).map_err(|err| err.to_string())?;
            Ok(json!(data.len()))
        }
        "mkdir" => {
            fs::create_dir_all(path).map_err(|err| err.to_string())?;
            Ok(json!(true))
        }
        "copy" => {
            let dest = request.destination.ok_or("fs.copy needs a destination")?;
            fs::copy(path, dest)
                .map(|bytes| json!(bytes))
                .map_err(|err| err.to_string())
        }
        "list" => {
            let mut found = Vec::new();
            for item in entries(path)? {
                let file_kind = kind(&item)?;
                found.push(record(&item, file_kind));
            }
            Ok(json!(found))
        }
        "walk" => {
            let mut found = Vec::new();
            walk(path, 0, &mut found)?;
            Ok(json!(found))
        }
        _ => Err(format!("unknown filesystem operation: {}", request.op)),
    }
}

/// Convert one trusted Lua JSON request into one JSON result.
pub fn serialized(request: String) -> String {
    let request = match serde_json::from_str::<Request>(&request) {
        Ok(request) => request,
        Err(err) => {
            return json!({"ok":false,"error":format!("invalid fs request: {err}")}).to_string();
        }
    };
    match perform(request) {
        Ok(value) => json!({"ok":true,"value":value}).to_string(),
        Err(err) => json!({"ok":false,"error":err}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_sentinel_and_rejected_path() {
        let ok: Value = serde_json::from_str(&serialized(
            r#"{"op":"exists","path":"/not-existing-ferrua-sentinel-123456789"}"#.into(),
        ))
        .unwrap();
        assert_eq!(ok["ok"], true);
        assert_eq!(ok["value"], false);
        let bad: Value =
            serde_json::from_str(&serialized(r#"{"op":"read","path":""}"#.into())).unwrap();
        assert_eq!(bad["ok"], false);
    }
}
