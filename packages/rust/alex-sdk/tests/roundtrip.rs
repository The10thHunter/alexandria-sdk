//! Round-trip tests for the SDK builders and pack/verify pipeline.

use std::io::Write;

use alex_sdk::manifest::{ComponentItem, InstallFlatten};
use alex_sdk::migrate::migrate_manifest;
use alex_sdk::{verify, Agent, Skill, Tool};

#[test]
fn agent_build_pack_verify_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("hello-0.1.0.aagent");

    let manifest = Agent::new("acme/hello", "0.1.0")
        .description("a tiny hello agent")
        .system_prompt("You are a helpful assistant.")
        .llm("claude-haiku")
        .pack(&out)
        .expect("pack");

    assert_eq!(manifest.name, "acme/hello");
    assert_eq!(manifest.version, "0.1.0");
    assert_eq!(manifest.schema_version, "2");

    let v = verify(&out).expect("verify");
    assert_eq!(v.name, "acme/hello");

    // Check llm field round-trips
    use alex_sdk::manifest::PackageConfig;
    if let PackageConfig::Agent(cfg) = &v.config {
        assert_eq!(cfg.llm.as_deref(), Some("claude-haiku"));
    } else {
        panic!("expected AgentConfig");
    }
}

#[test]
fn tool_stage_file_populates_sha256() {
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
        .stage_file(&bin_path, "dummy-bin", "bin/dummy-bin", true)
        .pack(&out)
        .expect("pack");

    let files = manifest.files.expect("files set");
    assert_eq!(files.len(), 1);
    let sha = files[0].sha256.as_ref().expect("sha256 populated");
    assert_eq!(sha.len(), 64, "sha256 should be 64 hex chars");
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));

    verify(&out).expect("verify");
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
            llm: None,
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
fn skill_builder_llm_field_round_trips() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("skill-0.1.0.atool");

    let manifest = Skill::new("acme/myskill", "0.1.0")
        .description("a skill with llm hint")
        .system_prompt("You are specialized.")
        .llm("claude-haiku")
        .tags(vec!["research".to_string()])
        .pack(&out)
        .expect("pack");

    use alex_sdk::manifest::PackageConfig;
    if let PackageConfig::Skill(cfg) = &manifest.config {
        assert_eq!(cfg.llm.as_deref(), Some("claude-haiku"));
        assert_eq!(cfg.tags.as_deref(), Some(["research".to_string()].as_slice()));
    } else {
        panic!("expected SkillConfig");
    }

    verify(&out).expect("verify");
}

#[test]
fn invalid_manifest_missing_description_fails_build() {
    let result = Agent::new("acme/sad", "0.1.0")
        .system_prompt("hi")
        .build();
    assert!(result.is_err(), "build should fail on empty description");
}

// ---------------------------------------------------------------------------
// Migration tests
// ---------------------------------------------------------------------------

#[test]
fn migrate_v1_agent_renames_model_to_llm() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/myagent",
        "version": "0.1.0",
        "kind": "agent",
        "description": "test agent",
        "config": {
            "kind": "agent",
            "system_prompt": "hello",
            "model": "claude-opus-4-7"
        }
    });
    let result = migrate_manifest(v1);
    assert!(result.errors.is_empty());
    assert_eq!(result.manifest["schema_version"], "2");
    assert_eq!(result.manifest["config"]["llm"], "claude-opus-4-7");
    assert!(result.manifest["config"].get("model").is_none());
    assert!(result.warnings.iter().any(|w| w.contains("model renamed")));
}

#[test]
fn migrate_v1_skill_renames_model_hint_to_llm() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/myskill",
        "version": "0.2.0",
        "kind": "skill",
        "description": "a skill",
        "config": {
            "kind": "skill",
            "system_prompt": "hi",
            "model_hint": "claude-haiku"
        }
    });
    let result = migrate_manifest(v1);
    assert!(result.errors.is_empty());
    assert_eq!(result.manifest["config"]["llm"], "claude-haiku");
    assert!(result.manifest["config"].get("model_hint").is_none());
}

#[test]
fn migrate_v1_bundle_converts_to_agent() {
    let v1 = serde_json::json!({
        "schema_version": "1",
        "name": "acme/mybundle",
        "version": "0.1.0",
        "kind": "bundle",
        "description": "a bundle",
        "config": {
            "kind": "bundle",
            "components": ["acme/foo@1.0.0", "acme/bar@2.0.0"]
        }
    });
    let result = migrate_manifest(v1);
    assert!(result.errors.is_empty());
    assert_eq!(result.manifest["kind"], "agent");
    let comps = result.manifest["components"].as_array().expect("components");
    assert_eq!(comps.len(), 2);
    assert_eq!(comps[0]["ref"], "acme/foo@1.0.0");
    assert!(result.warnings.iter().any(|w| w.contains("bundle converted")));
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
