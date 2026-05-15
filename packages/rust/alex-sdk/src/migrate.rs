//! Migration from v1 to v2 atool manifests.
//!
//! `migrate_manifest` takes a parsed v1 JSON value and returns a v2 JSON value
//! plus lists of warnings and errors. Callers write the result to `atool.json`.

use serde_json::{Map, Value};

/// Result of a migration pass.
pub struct MigrateResult {
    /// The (possibly transformed) manifest value.
    pub manifest: Value,
    /// Non-fatal issues — things that were changed but may require human review.
    pub warnings: Vec<String>,
    /// Fatal issues — the manifest cannot be migrated. If non-empty, the
    /// caller should abort and report errors without writing the output.
    pub errors: Vec<String>,
}

/// Migrate a v1 manifest JSON value to v2.
///
/// The input is consumed and transformed in-place where possible. Returns a
/// [`MigrateResult`] whose `manifest` field is the upgraded value.
pub fn migrate_manifest(mut v1: Value) -> MigrateResult {
    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let obj = match v1.as_object_mut() {
        Some(o) => o,
        None => {
            errors.push("manifest is not a JSON object".to_string());
            return MigrateResult { manifest: v1, warnings, errors };
        }
    };

    // Bump schema_version
    obj.insert("schema_version".to_string(), Value::String("2".to_string()));

    let kind = obj
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Handle removed kinds
    if kind == "llm-runtime" || kind == "llm-backend" {
        errors.push(format!(
            "kind '{}' has no v2 equivalent; register via `alexandria llm install` instead",
            kind
        ));
        return MigrateResult {
            manifest: Value::Object(obj.clone()),
            warnings,
            errors,
        };
    }

    if kind == "bundle" {
        obj.insert("kind".to_string(), Value::String("agent".to_string()));
        warnings.push("bundle converted to agent; add config.system_prompt before publishing".to_string());

        // Convert bundleConfig.components -> top-level components[]
        let old_comps: Vec<Value> = if let Some(cfg) = obj.get("config").and_then(|c| c.as_object()) {
            cfg.get("components")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let new_comps: Vec<Value> = old_comps
            .into_iter()
            .map(|ref_val| {
                let mut m = Map::new();
                m.insert("ref".to_string(), ref_val);
                Value::Object(m)
            })
            .collect();
        obj.insert("components".to_string(), Value::Array(new_comps));

        // Replace bundle config with minimal agent config
        let mut new_cfg = Map::new();
        new_cfg.insert("kind".to_string(), Value::String("agent".to_string()));
        new_cfg.insert(
            "system_prompt".to_string(),
            Value::String("TODO: add system_prompt".to_string()),
        );
        obj.insert("config".to_string(), Value::Object(new_cfg));
    }

    // Migrate config fields
    if let Some(cfg) = obj.get_mut("config").and_then(|c| c.as_object_mut()) {
        if let Some(model) = cfg.remove("model") {
            cfg.insert("llm".to_string(), model);
            warnings.push("config.model renamed to config.llm".to_string());
        }
        if let Some(model_hint) = cfg.remove("model_hint") {
            cfg.insert("llm".to_string(), model_hint);
            warnings.push("config.model_hint renamed to config.llm".to_string());
        }
        if cfg.remove("default_mode").is_some() {
            warnings.push("config.default_mode removed (swarm is always default)".to_string());
        }
        // Warn about default_port: 0
        if let Some(dp) = cfg.get("default_port") {
            let is_zero = dp.as_u64().map(|v| v == 0).unwrap_or(false)
                || dp.as_f64().map(|v| v == 0.0).unwrap_or(false);
            if is_zero {
                warnings.push(
                    "default_port was 0 (schema-invalid); set to a valid port 1-65535".to_string(),
                );
            }
        }
    }

    // Strip old signing fields at wrong locations
    let mut stripped_signing: Vec<String> = Vec::new();
    for field in &["signed_at", "key_fingerprint"] {
        if obj.remove(*field).is_some() {
            stripped_signing.push(field.to_string());
        }
    }
    // If signature present but not in v2 shape, remove it
    if let Some(sig) = obj.get("signature") {
        let has_v2_shape = sig.get("alg").is_some()
            && sig.get("key_fingerprint").is_some()
            && sig.get("value").is_some()
            && sig.get("scope").is_some();
        if !has_v2_shape {
            obj.remove("signature");
            stripped_signing.push("signature".to_string());
        }
    }
    if !stripped_signing.is_empty() {
        warnings.push(format!(
            "signing fields removed ({}); re-sign after migration",
            stripped_signing.join(", ")
        ));
    }

    // Warn about dependencies missing version
    if let Some(deps) = obj.get("dependencies").and_then(|d| d.as_array()) {
        for dep in deps {
            let version_missing = dep
                .get("version")
                .map(|v| v.is_null() || v.as_str().map(|s| s.is_empty()).unwrap_or(false))
                .unwrap_or(true);
            if version_missing {
                let name = dep
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("?")
                    .to_string();
                warnings.push(format!(
                    "dependency '{}' missing version field; add before publishing",
                    name
                ));
            }
        }
    }

    MigrateResult {
        manifest: Value::Object(obj.clone()),
        warnings,
        errors,
    }
}
