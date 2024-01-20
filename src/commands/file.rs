use arklib::{modify, modify_json, AtomicFile};

use crate::{
    append_json,
    parsers::{self, Format},
};

pub fn file_append(
    atomic_file: &AtomicFile,
    content: &str,
    format: Format,
) -> Result<(), String> {
    match format {
        parsers::Format::Raw => modify(&atomic_file, |current| {
            let mut combined_vec: Vec<u8> = current.to_vec();
            combined_vec.extend_from_slice(content.as_bytes());
            combined_vec
        })
        .map_err(|_| "ERROR: Could not append string".to_string()),
        parsers::Format::Json => {
            let values = parsers::key_value_to_str(&content)
                .map_err(|_| "ERROR: Could not parse json".to_string())?;

            append_json(&atomic_file, values.to_vec())
                .map_err(|_| "ERROR: Could not append json".to_string())
        }
    }
}

pub fn file_insert(
    atomic_file: &AtomicFile,
    content: &str,
    format: Format,
) -> Result<(), String> {
    match format {
        parsers::Format::Raw => {
            modify(&atomic_file, |_| content.as_bytes().to_vec())
                .map_err(|_| "ERROR: Could not insert string".to_string())
        }
        parsers::Format::Json => {
            let values = parsers::key_value_to_str(&content)
                .map_err(|_| "ERROR: Could not parse json".to_string())?;

            modify_json(
                &atomic_file,
                |current: &mut Option<serde_json::Value>| {
                    let mut new = serde_json::Map::new();
                    for (key, value) in &values {
                        new.insert(
                            key.clone(),
                            serde_json::Value::String(value.clone()),
                        );
                    }
                    *current = Some(serde_json::Value::Object(new));
                },
            )
            .map_err(|_| "ERROR:Could not insert json".to_string())
        }
    }
}
