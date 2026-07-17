//! Round-trip tests for the SDK builders and pack/verify pipeline
//! (EE-canonical schema v2).

use std::io::Write;

use alex_sdk::manifest::{
    ComponentItem, CredentialDecl, EnvDecl, InstallFlatten, LockEntry, PackageConfig, PackageDep,
    PromptMode, Rotation, WireTransport,
};
use alex_sdk::migrate::migrate_manifest;
use alex_sdk::{validate, verify, Agent, Bundle, Skill, Tool};

#[test]
fn agent_build_pack_verify_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("hello-0.1.0.aagent");

    let manifest = Agent::new("acme/hello", "0.1.0")
        .description("a tiny hello agent")
        .system_prompt("You are a helpful assistant.")
        .model("claude-haiku")
        .history_limit(50)
        .pack(&out)
        .expect("pack");

    assert_eq!(manifest.name, "acme/hello");
    assert_eq!(manifest.version, "0.1.0");
    assert_eq!(manifest.schema_version, "2");

    let v = verify(&out).expect("verify");
    assert_eq!(v.name, "acme/hello");

    if let PackageConfig::Aagent(cfg) = &v.config {
        assert_eq!(cfg.model.as_deref(), Some("claude-haiku"));
    } else {
        panic!("expected AagentConfig");
    }
}

#[test]
fn tool_defaults_to_atool_and_populates_sha256() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin_path = dir.path().join("dummy-bin");
    {
        let mut f = std::fs::File::create(&bin_path).expect("create bin");
        f.write_all(b"#!/bin/sh\necho hi\n").expect("write bin");
    }

    let out = dir.path().join("dummy-tool-0.1.0.atool");
    let manifest = Tool::new("acme/dummy", "0.1.0")
        .description("dummy tool")
        .binary("./dummy-bin")
        .interface_major(2)
        .stage_file(&bin_path, "dummy-bin", "bin/dummy-bin", true)
        .pack(&out)
        .expect("pack");

    assert_eq!(manifest.kind, alex_sdk::manifest::Kind::Atool);
    if let PackageConfig::Atool(cfg) = &manifest.config {
        assert_eq!(cfg.interface_major, Some(2));
    } else {
        panic!("expected AtoolConfig");
    }

    let files = manifest.files.expect("files set");
    assert_eq!(files.len(), 1);
    let sha = files[0].sha256.as_ref().expect("sha256 populated");
    assert_eq!(sha.len(), 64, "sha256 should be 64 hex chars");
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));

    verify(&out).expect("verify");
}

#[test]
fn tool_credential_and_env_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bin_path = dir.path().join("gh-bin");
    {
        let mut f = std::fs::File::create(&bin_path).expect("create bin");
        f.write_all(b"#!/bin/sh\nexit 0\n").expect("write bin");
    }

    let out = dir.path().join("github-0.1.0.atool");
    Tool::new("acme/github", "0.1.0")
        .description("GitHub tool with a declared credential")
        .binary("bin/gh")
        .credential(
            CredentialDecl::new("GITHUB_PERSONAL_ACCESS_TOKEN")
                .required(true)
                .description("GitHub PAT"),
        )
        .env_var(EnvDecl::new("GITHUB_HOST").default_value("github.com"))
        .stage_file(&bin_path, "bin/gh", "tools/gh/bin/gh", true)
        .pack(&out)
        .expect("pack");

    let v = verify(&out).expect("verify");
    if let PackageConfig::Atool(cfg) = &v.config {
        let creds = cfg.credentials.as_ref().expect("credentials set");
        assert_eq!(creds.len(), 1);
        assert_eq!(creds[0].env, "GITHUB_PERSONAL_ACCESS_TOKEN");
        assert_eq!(creds[0].required, Some(true));
        assert_eq!(creds[0].secret, Some(true), "secret defaults to true");
        assert_eq!(
            creds[0].rotation,
            Some(Rotation::Respawn),
            "rotation defaults to respawn"
        );
        assert_eq!(creds[0].description.as_deref(), Some("GitHub PAT"));
        let envs = cfg.env.as_ref().expect("env set");
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].name, "GITHUB_HOST");
        assert_eq!(envs[0].default.as_deref(), Some("github.com"));
    } else {
        panic!("expected AtoolConfig");
    }
}

#[test]
fn credential_with_illegal_env_name_is_rejected() {
    let manifest = serde_json::json!({
        "schema_version": "2",
        "name": "acme/bad-cred",
        "version": "0.1.0",
        "kind": "atool",
        "description": "atool with an illegal credential env name",
        "config": {
            "kind": "atool",
            "binary": "bin/x",
            "credentials": [{ "env": "9-not-valid", "required": true }]
        }
    });
    assert!(
        validate(&manifest).is_err(),
        "illegal env var name must be rejected"
    );
}

#[test]
fn code_less_tool_round_trips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("delegate-0.1.0.atool");

    let schema = serde_json::json!({
        "type": "object",
        "required": ["objective"],
        "properties": {
            "objective": { "type": "string" },
            "acceptance": { "type": "array", "items": { "type": "string" } }
        }
    });

    let manifest = Tool::new("acme/delegate", "0.1.0")
        .description("code-less delegation tool")
        .native_handler("emit_trigger")
        .input_schema(schema)
        .pack(&out)
        .expect("pack");

    assert_eq!(manifest.kind, alex_sdk::manifest::Kind::Atool);
    if let PackageConfig::Atool(cfg) = &manifest.config {
        assert_eq!(cfg.native_handler, "emit_trigger");
        assert!(cfg.binary.is_empty(), "code-less tool omits binary");
        assert!(cfg.input_schema.is_some(), "input_schema present");
    } else {
        panic!("expected AtoolConfig");
    }

    // Serialised form must omit `binary` entirely (schema minLength would reject "").
    let serialised = serde_json::to_string(&manifest).expect("serialise");
    assert!(
        !serialised.contains("\"binary\""),
        "code-less tool must not serialise a binary field: {serialised}"
    );

    verify(&out).expect("verify");
}

#[test]
fn code_less_tool_without_input_schema_is_rejected() {
    let manifest = serde_json::json!({
        "schema_version": "2",
        "name": "acme/bad-native",
        "version": "0.1.0",
        "kind": "atool",
        "description": "code-less tool missing its input_schema",
        "config": { "kind": "atool", "native_handler": "emit_trigger" }
    });
    assert!(
        validate(&manifest).is_err(),
        "code-less tool must declare input_schema"
    );
}

#[test]
fn atool_with_neither_binary_nor_handler_is_rejected() {
    let manifest = serde_json::json!({
        "schema_version": "2",
        "name": "acme/empty-tool",
        "version": "0.1.0",
        "kind": "atool",
        "description": "atool with neither binary nor native_handler",
        "config": { "kind": "atool" }
    });
    assert!(
        validate(&manifest).is_err(),
        "one of binary/native_handler is required"
    );
}

#[test]
fn tool_transport_http_retaxes_to_mcp() {
    let manifest = Tool::new("acme/mcptool", "0.1.0")
        .description("mcp daemon")
        .binary("bin/x")
        .port(7800)
        .transport(WireTransport::Http)
        .build()
        .expect("build");
    assert_eq!(manifest.kind, alex_sdk::manifest::Kind::Mcp);
    match &manifest.config {
        PackageConfig::Mcp(_) => {}
        _ => panic!("expected McpConfig"),
    }
}

#[test]
fn tool_transport_grpc_stays_atool() {
    let manifest = Tool::new("acme/g", "0.1.0")
        .description("grpc tool")
        .binary("bin/g")
        .transport(WireTransport::Grpc)
        .build()
        .expect("build");
    assert_eq!(manifest.kind, alex_sdk::manifest::Kind::Atool);
    match &manifest.config {
        PackageConfig::Atool(_) => {}
        _ => panic!("expected AtoolConfig"),
    }
}

#[test]
fn agent_with_ref_component_round_trips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("parent-0.1.0.aagent");

    let manifest = Agent::new("acme/parent", "0.1.0")
        .description("parent agent")
        .system_prompt("You orchestrate via refs.")
        .ref_component("acme/some-tool@1.0.0")
        .ref_component("acme/some-agent@2.0.0")
        .pack(&out)
        .expect("pack");

    let comps = manifest.components.expect("components");
    assert_eq!(comps.len(), 2);
    match &comps[0] {
        ComponentItem::Ref(r) => assert_eq!(r.ref_target, "acme/some-tool@1.0.0"),
        _ => panic!("expected ref component"),
    }

    verify(&out).expect("verify");
}

#[test]
fn agent_with_inline_sub_agent_round_trips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("parent-inline-0.1.0.aagent");

    let child = Agent::new("acme/child", "0.1.0")
        .description("child agent")
        .system_prompt("You are a child.");

    let manifest = Agent::new("acme/parent", "0.1.0")
        .description("parent agent with inline component")
        .system_prompt("You orchestrate.")
        .component("child-agent", "acme/child@0.1.0", child)
        .expect("component")
        .pack(&out)
        .expect("pack");

    let comps = manifest.components.expect("components");
    assert_eq!(comps.len(), 1);
    match &comps[0] {
        ComponentItem::Inline(inline) => {
            assert_eq!(inline.name, "child-agent");
            assert_eq!(inline.id, "acme/child@0.1.0");
        }
        _ => panic!("expected inline component"),
    }

    verify(&out).expect("verify");
}

#[test]
fn agent_extends_and_lockfile_round_trips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("child-0.1.0.aagent");

    let manifest = Agent::new("acme/child", "0.1.0")
        .description("child agent extending a base")
        .system_prompt("You extend a base agent.")
        .prompt_mode(PromptMode::Append)
        .extend(PackageDep {
            name: "acme/base-agent".to_string(),
            version: Some("1.0.0".to_string()),
        })
        .lock(LockEntry {
            name: "web-search".to_string(),
            interface_major: 2,
            contract_hash: None,
        })
        .pack(&out)
        .expect("pack");

    let extends = manifest.extends.expect("extends");
    assert_eq!(extends.len(), 1);
    assert_eq!(extends[0].name, "acme/base-agent");
    let lock = manifest.lockfile.expect("lockfile");
    assert_eq!(lock[0].name, "web-search");
    assert_eq!(lock[0].interface_major, 2);

    verify(&out).expect("verify");
}

#[test]
fn agent_with_flatten_rules_round_trips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("flat-0.1.0.aagent");

    let manifest = Agent::new("acme/flat", "0.1.0")
        .description("agent with flatten")
        .system_prompt("You merge sub-agents.")
        .ref_component("acme/sub@1.0.0")
        .flatten(InstallFlatten {
            system_prompt: Some("concat".to_string()),
            allowed_tools: Some("union".to_string()),
            model: None,
            history_limit: None,
        })
        .pack(&out)
        .expect("pack");

    let install = manifest.install.expect("install block");
    let flatten = install.flatten.expect("flatten");
    assert_eq!(flatten.system_prompt.as_deref(), Some("concat"));
    assert_eq!(flatten.allowed_tools.as_deref(), Some("union"));

    verify(&out).expect("verify");
}

#[test]
fn skill_emits_aagent_with_model() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("skill-0.1.0.aagent");

    let manifest = Skill::new("acme/myskill", "0.1.0")
        .description("a prompt-only skill")
        .system_prompt("You are specialized.")
        .model("claude-haiku")
        .pack(&out)
        .expect("pack");

    assert_eq!(manifest.kind, alex_sdk::manifest::Kind::Aagent);
    if let PackageConfig::Aagent(cfg) = &manifest.config {
        assert_eq!(cfg.model.as_deref(), Some("claude-haiku"));
    } else {
        panic!("expected AagentConfig");
    }

    verify(&out).expect("verify");
}

#[test]
fn bundle_build_pack_verify_round_trips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("doer-0.1.0.atool");

    let manifest = Bundle::new("essentials/doer", "0.1.0")
        .description("The doer stance: do the work, then submit or report blocked.")
        .tool("essentials/submit-deliverable")
        .tool("essentials/report-blocked")
        .pack(&out)
        .expect("pack");

    assert_eq!(manifest.kind, alex_sdk::manifest::Kind::Bundle);
    if let PackageConfig::Bundle(cfg) = &manifest.config {
        assert_eq!(
            cfg.tools,
            vec![
                "essentials/submit-deliverable".to_string(),
                "essentials/report-blocked".to_string()
            ]
        );
    } else {
        panic!("expected BundleConfig");
    }

    // A bundle is pure composition — no binary / system_prompt on the wire.
    let serialised = serde_json::to_string(&manifest).expect("serialise");
    assert!(
        !serialised.contains("\"binary\"") && !serialised.contains("system_prompt"),
        "bundle must be pure composition: {serialised}"
    );

    verify(&out).expect("verify");
}

#[test]
fn bundle_with_empty_tools_is_rejected() {
    let manifest = serde_json::json!({
        "schema_version": "2",
        "name": "essentials/empty-bundle",
        "version": "0.1.0",
        "kind": "bundle",
        "description": "a bundle grouping nothing",
        "config": { "kind": "bundle", "tools": [] }
    });
    assert!(
        validate(&manifest).is_err(),
        "bundle with no tools must be rejected (minItems:1)"
    );
}

#[test]
fn invalid_manifest_missing_description_fails_build() {
    let result = Agent::new("acme/sad", "0.1.0").system_prompt("hi").build();
    assert!(result.is_err(), "build should fail on empty description");
}

// ---------------------------------------------------------------------------
// Migration tests
// ---------------------------------------------------------------------------

#[test]
fn migrate_v1_tool_becomes_mcp_by_default() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/mytool",
        "version": "0.1.0",
        "kind": "tool",
        "description": "http tool",
        "config": { "kind": "tool", "binary": "bin/x", "transport": "http" }
    });
    let result = migrate_manifest(v1);
    assert!(result.errors.is_empty());
    assert_eq!(result.manifest["kind"], "mcp");
    assert_eq!(result.manifest["config"]["kind"], "mcp");
}

#[test]
fn migrate_v1_grpc_tool_becomes_atool() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/mytool",
        "version": "0.1.0",
        "kind": "tool",
        "description": "grpc tool",
        "config": { "kind": "tool", "binary": "bin/x", "transport": "grpc" }
    });
    let result = migrate_manifest(v1);
    assert!(result.errors.is_empty());
    assert_eq!(result.manifest["kind"], "atool");
    assert_eq!(result.manifest["config"]["kind"], "atool");
}

#[test]
fn migrate_v1_agent_keeps_config_model() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/myagent",
        "version": "0.1.0",
        "kind": "agent",
        "description": "test agent",
        "config": { "kind": "agent", "system_prompt": "hello", "model": "claude-opus-4-7" }
    });
    let result = migrate_manifest(v1);
    assert!(result.errors.is_empty());
    assert_eq!(result.manifest["schema_version"], "2");
    assert_eq!(result.manifest["kind"], "aagent");
    assert_eq!(result.manifest["config"]["kind"], "aagent");
    assert_eq!(result.manifest["config"]["model"], "claude-opus-4-7");
    assert!(result.manifest["config"].get("llm").is_none());
}

#[test]
fn migrate_intermediate_llm_folds_to_model() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/myagent",
        "version": "0.1.0",
        "kind": "agent",
        "description": "test agent",
        "config": { "kind": "agent", "system_prompt": "hello", "llm": "claude-opus-4-7" }
    });
    let result = migrate_manifest(v1);
    assert!(result.errors.is_empty());
    assert_eq!(result.manifest["config"]["model"], "claude-opus-4-7");
    assert!(result.manifest["config"].get("llm").is_none());
    assert!(result
        .warnings
        .iter()
        .any(|w| w.contains("llm renamed to config.model")));
}

#[test]
fn migrate_v1_skill_to_aagent_drops_tags() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/myskill",
        "version": "0.2.0",
        "kind": "skill",
        "description": "a skill",
        "config": { "kind": "skill", "system_prompt": "hi", "model_hint": "claude-haiku", "tags": ["a", "b"] }
    });
    let result = migrate_manifest(v1);
    assert!(result.errors.is_empty());
    assert_eq!(result.manifest["kind"], "aagent");
    assert_eq!(result.manifest["config"]["kind"], "aagent");
    assert_eq!(result.manifest["config"]["model"], "claude-haiku");
    assert!(result.manifest["config"].get("model_hint").is_none());
    assert!(result.manifest["config"].get("tags").is_none());
    assert!(result.warnings.iter().any(|w| w.contains("tags removed")));
}

#[test]
fn migrate_v1_bundle_converts_to_aagent() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/mybundle",
        "version": "0.1.0",
        "kind": "bundle",
        "description": "a bundle",
        "config": { "kind": "bundle", "components": ["acme/foo@1.0.0", "acme/bar@2.0.0"] }
    });
    let result = migrate_manifest(v1);
    assert!(result.errors.is_empty());
    assert_eq!(result.manifest["kind"], "aagent");
    let comps = result.manifest["components"]
        .as_array()
        .expect("components");
    assert_eq!(comps.len(), 2);
    assert_eq!(comps[0]["ref"], "acme/foo@1.0.0");
    assert!(result
        .warnings
        .iter()
        .any(|w| w.contains("bundle converted")));
}

#[test]
fn migrate_llm_runtime_errors() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/runtime",
        "version": "0.1.0",
        "kind": "llm-runtime",
        "description": "a runtime",
        "config": {"kind": "llm-runtime"}
    });
    let result = migrate_manifest(v1);
    assert!(!result.errors.is_empty());
    assert!(result.errors[0].contains("llm-runtime"));
}

#[test]
fn migrate_llm_backend_errors() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/backend",
        "version": "0.1.0",
        "kind": "llm-backend",
        "description": "a backend",
        "config": {"kind": "llm-backend"}
    });
    let result = migrate_manifest(v1);
    assert!(!result.errors.is_empty());
    assert!(result.errors[0].contains("llm-backend"));
}
