package alexsdk

// MigrateManifest upgrades a v1 manifest map to v2.
// Returns (manifest, warnings, errors). If errors is non-empty, migration
// cannot proceed (un-migratable kind). Warnings are non-fatal issues.
//
// Callers should write the returned manifest to atool.json after migration.
func MigrateManifest(v1 map[string]interface{}) (manifest map[string]interface{}, warnings []string, errors []string) {
	m := shallowCopy(v1)

	// Bump schema_version
	m["schema_version"] = "2"

	kind, _ := m["kind"].(string)

	// Handle removed kinds
	if kind == "llm-runtime" || kind == "llm-backend" {
		errors = append(errors,
			"kind '"+kind+"' has no v2 equivalent; register via `alexandria llm install` instead",
		)
		return m, warnings, errors
	}

	if kind == "bundle" {
		m["kind"] = "agent"
		warnings = append(warnings, "bundle converted to agent; add config.system_prompt before publishing")

		// Convert bundleConfig.components -> top-level components[]
		if cfg, ok := m["config"].(map[string]interface{}); ok {
			if oldComponents, ok := cfg["components"].([]interface{}); ok {
				newComps := make([]interface{}, 0, len(oldComponents))
				for _, ref := range oldComponents {
					newComps = append(newComps, map[string]interface{}{"ref": ref})
				}
				m["components"] = newComps
			}
		}
		// Replace bundle config with minimal agent config
		m["config"] = map[string]interface{}{
			"kind":          "agent",
			"system_prompt": "TODO: add system_prompt",
		}
	}

	// Migrate config fields
	if cfg, ok := m["config"].(map[string]interface{}); ok {
		if model, ok := cfg["model"]; ok {
			cfg["llm"] = model
			delete(cfg, "model")
			warnings = append(warnings, "config.model renamed to config.llm")
		}
		if modelHint, ok := cfg["model_hint"]; ok {
			cfg["llm"] = modelHint
			delete(cfg, "model_hint")
			warnings = append(warnings, "config.model_hint renamed to config.llm")
		}
		if _, ok := cfg["default_mode"]; ok {
			delete(cfg, "default_mode")
			warnings = append(warnings, "config.default_mode removed (swarm is always default)")
		}
		// Warn about default_port: 0
		if dp, ok := cfg["default_port"]; ok {
			switch v := dp.(type) {
			case float64:
				if v == 0 {
					warnings = append(warnings, "default_port was 0 (schema-invalid); set to a valid port 1-65535")
				}
			case int:
				if v == 0 {
					warnings = append(warnings, "default_port was 0 (schema-invalid); set to a valid port 1-65535")
				}
			}
		}
		m["config"] = cfg
	}

	// Strip old signing fields at wrong locations
	var strippedSigning []string
	for _, field := range []string{"signed_at", "key_fingerprint"} {
		if _, ok := m[field]; ok {
			delete(m, field)
			strippedSigning = append(strippedSigning, field)
		}
	}
	// If signature present but not in v2 shape, remove it
	if sig, ok := m["signature"]; ok {
		sigMap, isMap := sig.(map[string]interface{})
		hasV2Shape := isMap &&
			sigMap["alg"] != nil &&
			sigMap["key_fingerprint"] != nil &&
			sigMap["value"] != nil &&
			sigMap["scope"] != nil
		if !hasV2Shape {
			delete(m, "signature")
			strippedSigning = append(strippedSigning, "signature")
		}
	}
	if len(strippedSigning) > 0 {
		msg := "signing fields removed ("
		for i, f := range strippedSigning {
			if i > 0 {
				msg += ", "
			}
			msg += f
		}
		msg += "); re-sign after migration"
		warnings = append(warnings, msg)
	}

	// Warn about dependencies missing version
	if deps, ok := m["dependencies"].([]interface{}); ok {
		for _, dep := range deps {
			if depMap, ok := dep.(map[string]interface{}); ok {
				if depMap["version"] == nil || depMap["version"] == "" {
					name, _ := depMap["name"].(string)
					if name == "" {
						name = "?"
					}
					warnings = append(warnings, "dependency '"+name+"' missing version field; add before publishing")
				}
			}
		}
	}

	return m, warnings, errors
}

func shallowCopy(m map[string]interface{}) map[string]interface{} {
	out := make(map[string]interface{}, len(m))
	for k, v := range m {
		out[k] = v
	}
	return out
}
