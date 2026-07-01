package alexsdk

// MigrateManifest upgrades a v1 manifest map to the EE-canonical v2 taxonomy.
// Returns (manifest, warnings, errors). If errors is non-empty, migration
// cannot proceed (un-migratable kind). Warnings are non-fatal issues.
//
// Kind remap:  tool -> mcp|atool (by transport: grpc=>atool, else mcp);
//
//	skill -> aagent;  agent -> aagent;  bundle -> aagent.
//
// Field remap: model stays model (EE uses `model`); llm/model_hint -> model;
//
//	aagent tags dropped (no such field in EE AagentConfig).
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
			"kind '"+kind+"' has no v2 equivalent; register a model via "+
				"`alexandria install <name> --model` (.amodel) instead",
		)
		return m, warnings, errors
	}

	switch kind {
	case "bundle":
		// A bundle collapses to an aagent orchestrator carrying components[] refs.
		m["kind"] = "aagent"
		warnings = append(warnings, "bundle converted to aagent; add config.system_prompt before publishing")
		if cfg, ok := m["config"].(map[string]interface{}); ok {
			if oldComponents, ok := cfg["components"].([]interface{}); ok {
				newComps := make([]interface{}, 0, len(oldComponents))
				for _, ref := range oldComponents {
					newComps = append(newComps, map[string]interface{}{"ref": ref})
				}
				m["components"] = newComps
			}
		}
		m["config"] = map[string]interface{}{
			"kind":          "aagent",
			"system_prompt": "TODO: add system_prompt",
		}
	case "tool":
		// A v1 tool becomes mcp (MCP JSON-RPC/SSE) or atool (native gRPC),
		// discriminated by transport. Default (no transport) is MCP over http.
		cfg, _ := m["config"].(map[string]interface{})
		transport := ""
		if cfg != nil {
			transport, _ = cfg["transport"].(string)
		}
		newKind := "mcp"
		if transport == "grpc" {
			newKind = "atool"
		}
		m["kind"] = newKind
		if cfg != nil {
			cfg["kind"] = newKind
			m["config"] = cfg
		}
		if newKind == "atool" {
			warnings = append(warnings, "kind 'tool' with transport=grpc migrated to kind 'atool' (native ToolService)")
		} else {
			warnings = append(warnings, "kind 'tool' migrated to kind 'mcp' (MCP JSON-RPC/SSE)")
		}
	case "skill":
		// EE has no standalone skill kind — a skill is reusable prompt text that
		// ships as an aagent whose content is its system_prompt.
		m["kind"] = "aagent"
		if cfg, ok := m["config"].(map[string]interface{}); ok {
			cfg["kind"] = "aagent"
			m["config"] = cfg
		}
		warnings = append(warnings, "kind 'skill' migrated to kind 'aagent' (skills live in aagent.system_prompt)")
	case "agent":
		m["kind"] = "aagent"
		if cfg, ok := m["config"].(map[string]interface{}); ok {
			cfg["kind"] = "aagent"
			m["config"] = cfg
		}
	}

	// Migrate config fields to EE serde names.
	if cfg, ok := m["config"].(map[string]interface{}); ok {
		// EE uses `model`; the intermediate SDK-v2 field `llm` folds back to it.
		if llm, ok := cfg["llm"]; ok {
			cfg["model"] = llm
			delete(cfg, "llm")
			warnings = append(warnings, "config.llm renamed to config.model")
		}
		if modelHint, ok := cfg["model_hint"]; ok {
			cfg["model"] = modelHint
			delete(cfg, "model_hint")
			warnings = append(warnings, "config.model_hint renamed to config.model")
		}
		if _, ok := cfg["default_mode"]; ok {
			delete(cfg, "default_mode")
			warnings = append(warnings, "config.default_mode removed (swarm is always default)")
		}
		if _, ok := cfg["tags"]; ok {
			delete(cfg, "tags")
			warnings = append(warnings, "config.tags removed (EE aagent has no tags field)")
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
