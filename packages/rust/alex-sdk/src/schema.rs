//! JSON Schema validation of an `atool.json` manifest.
//!
//! The canonical schema lives at the workspace root (`schemas/atool.schema.json`)
//! and is `include_str!`'d at build time, so the resulting library has zero
//! runtime filesystem dependency.

use std::sync::OnceLock;

use jsonschema::{Draft, JSONSchema};
use serde_json::Value;

use crate::Error;

const SCHEMA_JSON: &str = include_str!("../../../../schemas/atool.schema.json");

fn compiled() -> &'static JSONSchema {
    static CELL: OnceLock<JSONSchema> = OnceLock::new();
    CELL.get_or_init(|| {
        // SAFETY: schema is bundled and known good at build time. We panic
        // here rather than threading a Result through every call site —
        // a broken schema is an unrecoverable build-time bug.
        let value: Value = serde_json::from_str(SCHEMA_JSON)
            .expect("bundled atool.schema.json is valid JSON");
        JSONSchema::options()
            .with_draft(Draft::Draft202012)
            .compile(&value)
            .expect("bundled atool.schema.json is a valid JSON Schema")
    })
}

/// A single schema validation error, flattened for ergonomic display.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// Validate a manifest `Value` against the bundled schema.
///
/// Returns `Ok(())` on success, or a list of every violation found.
pub fn validate(value: &Value) -> std::result::Result<(), Vec<ValidationError>> {
    let schema = compiled();
    match schema.validate(value) {
        Ok(()) => Ok(()),
        Err(errors) => {
            let collected: Vec<ValidationError> = errors
                .map(|e| {
                    let path = e.instance_path.to_string();
                    let path = if path.is_empty() {
                        "(root)".to_string()
                    } else {
                        path
                    };
                    ValidationError {
                        path,
                        message: e.to_string(),
                    }
                })
                .collect();
            Err(collected)
        }
    }
}

/// Validate and return an SDK [`Error::Schema`] on failure. Useful in the
/// builder/pack hot paths.
pub fn assert_valid(value: &Value) -> Result<(), Error> {
    match validate(value) {
        Ok(()) => Ok(()),
        Err(errors) => {
            let mut buf = String::new();
            for e in &errors {
                buf.push_str("  ");
                buf.push_str(&e.path);
                buf.push_str(": ");
                buf.push_str(&e.message);
                buf.push('\n');
            }
            // strip trailing newline for clean Display
            let trimmed = buf.trim_end_matches('\n').to_string();
            Err(Error::Schema(trimmed))
        }
    }
}
