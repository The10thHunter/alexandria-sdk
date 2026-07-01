"""Tests for the Alexandria Python SDK builders, packing, and verification."""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

from alexandria_sdk import Agent, Skill, Tool, inspect, pack, verify
from alexandria_sdk.cli import _migrate_manifest
from alexandria_sdk.schema import validate


HEX64 = re.compile(r"^[a-f0-9]{64}$")


def test_agent_pack_verify_round_trip(tmp_path: Path) -> None:
    out = tmp_path / "research-0.1.0.aagent"
    manifest = (
        Agent("research", "0.1.0")
        .description("Research assistant")
        .system_prompt("You are a research assistant.")
        .model("claude-opus-4-7")
        .history_limit(50)
        .pack(out)
    )
    assert out.is_file()
    assert manifest["kind"] == "aagent"
    assert manifest["schema_version"] == "2"
    assert manifest["config"]["system_prompt"].startswith("You are")
    assert manifest["config"]["model"] == "claude-opus-4-7"

    verified = verify(out)
    assert verified["name"] == "research"
    assert verified["version"] == "0.1.0"

    info = inspect(out)
    names = [f["name"] for f in info.files]
    assert "atool.json" in names


def test_tool_defaults_to_atool(tmp_path: Path) -> None:
    bin_src = tmp_path / "mytool"
    bin_src.write_bytes(b"#!/bin/sh\necho hi\n")

    out = tmp_path / "mytool-1.2.3.atool"
    manifest = (
        Tool("mytool", "1.2.3")
        .description("a tiny native gRPC tool")
        .binary("bin/mytool")
        .interface_major(2)
        .stage_file(
            str(bin_src),
            archive_path="bin/mytool",
            install_path="bin/mytool",
            executable=True,
        )
        .pack(out)
    )

    assert out.is_file()
    assert manifest["kind"] == "atool"
    assert manifest["config"]["kind"] == "atool"
    assert manifest["config"]["interface_major"] == 2
    assert manifest["schema_version"] == "2"
    files = manifest["files"]
    assert len(files) == 1
    entry = files[0]
    assert entry["archive_path"] == "bin/mytool"
    assert entry["executable"] is True
    assert HEX64.match(entry["sha256"]), entry["sha256"]

    verify(out)


def test_tool_transport_http_retaxes_to_mcp(tmp_path: Path) -> None:
    bin_src = tmp_path / "mcptool"
    bin_src.write_bytes(b"#!/bin/sh\necho hi\n")

    out = tmp_path / "mcptool-0.1.0.atool"
    manifest = (
        Tool("acme/mcptool", "0.1.0")
        .description("an mcp daemon")
        .binary("bin/mcptool")
        .port(7800)
        .transport("http")
        .stage_file(str(bin_src), "bin/mcptool", "bin/mcptool", executable=True)
        .pack(out)
    )
    assert manifest["kind"] == "mcp"
    assert manifest["config"]["kind"] == "mcp"
    assert manifest["config"]["transport"] == "http"
    verify(out)


def test_tool_transport_grpc_stays_atool() -> None:
    manifest = (
        Tool("acme/g", "0.1.0")
        .description("grpc tool")
        .binary("bin/g")
        .transport("grpc")
        .build()
    )
    assert manifest["kind"] == "atool"
    assert manifest["config"]["kind"] == "atool"


def test_agent_with_inline_component(tmp_path: Path) -> None:
    child = (
        Agent("child", "0.1.0")
        .description("child agent")
        .system_prompt("You are a child agent.")
    )
    parent = (
        Agent("parent", "0.1.0")
        .description("parent with child")
        .system_prompt("You orchestrate.")
        .component("my-child", "acme/child@0.1.0", child)
    )
    out = tmp_path / "parent-0.1.0.aagent"
    manifest = parent.pack(out)

    assert manifest["components"] is not None
    assert len(manifest["components"]) == 1
    comp = manifest["components"][0]
    assert comp["name"] == "my-child"
    assert comp["id"] == "acme/child@0.1.0"
    assert comp["kind"] == "aagent"

    verified = verify(out)
    assert verified["components"][0]["name"] == "my-child"


def test_agent_extends_and_lockfile_round_trip(tmp_path: Path) -> None:
    out = tmp_path / "child-0.1.0.aagent"
    manifest = (
        Agent("acme/child", "0.1.0")
        .description("child agent extending a base")
        .system_prompt("You extend a base agent.")
        .prompt_mode("append")
        .extend({"name": "acme/base-agent", "version": "1.0.0"})
        .lock({"name": "web-search", "interface_major": 2})
        .pack(out)
    )
    assert manifest["extends"] == [{"name": "acme/base-agent", "version": "1.0.0"}]
    assert manifest["lockfile"][0]["name"] == "web-search"
    assert manifest["lockfile"][0]["interface_major"] == 2
    assert manifest["config"]["prompt_mode"] == "append"
    verify(out)


def test_validation_rejects_extends_on_atool() -> None:
    manifest = {
        "schema_version": "2",
        "name": "acme/bad-tool",
        "version": "0.1.0",
        "kind": "atool",
        "description": "atool that wrongly carries extends",
        "config": {"kind": "atool", "binary": "bin/x"},
        "extends": [{"name": "acme/base", "version": "1.0.0"}],
    }
    ok, _errors = validate(manifest)
    assert not ok, "extends must be rejected on atool"


def test_skill_embeds_inline_as_aagent(tmp_path: Path) -> None:
    skill = (
        Skill("acme/skill", "0.1.0")
        .description("prompt skill")
        .system_prompt("Reusable prompt text.")
    )
    parent = (
        Agent("acme/parent-skill", "0.1.0")
        .description("parent embedding a skill")
        .system_prompt("You compose a skill.")
        .component("my-skill", "acme/skill@0.1.0", skill)
    )
    out = tmp_path / "parent-skill-0.1.0.aagent"
    manifest = parent.pack(out)
    assert manifest["components"][0]["kind"] == "aagent"


def test_agent_with_ref_component(tmp_path: Path) -> None:
    agent = (
        Agent("orchestrator", "1.0.0")
        .description("orchestrator")
        .system_prompt("You use tools.")
        .ref("acme/some-tool@1.0.0")
        .ref("acme/some-agent@2.0.0")
    )
    out = tmp_path / "orchestrator-1.0.0.aagent"
    manifest = agent.pack(out)
    assert len(manifest["components"]) == 2
    assert manifest["components"][0] == {"ref": "acme/some-tool@1.0.0"}


def test_agent_with_flatten_rules(tmp_path: Path) -> None:
    agent = (
        Agent("flattened", "0.1.0")
        .description("agent with flatten")
        .system_prompt("You merge sub-agents.")
        .ref("acme/sub@1.0.0")
        .flatten({"system_prompt": "concat", "allowed_tools": "union"})
    )
    out = tmp_path / "flattened-0.1.0.aagent"
    manifest = agent.pack(out)
    assert manifest["install"]["flatten"]["system_prompt"] == "concat"
    assert manifest["install"]["flatten"]["allowed_tools"] == "union"


def test_validation_rejects_components_on_tool() -> None:
    manifest = {
        "schema_version": "2",
        "name": "acme/bad-tool",
        "version": "0.1.0",
        "kind": "atool",
        "description": "tool with components",
        "config": {"kind": "atool", "binary": "bin/x"},
        "components": [{"ref": "acme/foo@1.0.0"}],
    }
    ok, _errors = validate(manifest)
    assert not ok, "should reject components on atool"


def test_validation_rejects_inline_tool_in_components() -> None:
    manifest = {
        "schema_version": "2",
        "name": "acme/bad-agent",
        "version": "0.1.0",
        "kind": "aagent",
        "description": "agent with inline tool",
        "config": {"kind": "aagent", "system_prompt": "hi"},
        "components": [
            {
                "name": "my-tool",
                "id": "acme/mytool@1.0.0",
                "kind": "atool",
                "config": {"kind": "atool", "binary": "bin/x"},
            }
        ],
    }
    ok, _errors = validate(manifest)
    assert not ok, "should reject inline tool in components"


def test_validation_accepts_ref_to_tool_in_agent_components() -> None:
    manifest = {
        "schema_version": "2",
        "name": "acme/agent-with-tool-ref",
        "version": "0.1.0",
        "kind": "aagent",
        "description": "agent that refs a tool",
        "config": {"kind": "aagent", "system_prompt": "hi"},
        "components": [{"ref": "acme/some-tool@1.0.0"}],
    }
    ok, errors = validate(manifest)
    assert ok, f"should accept ref in components: {errors}"


def test_signature_block_accepted() -> None:
    manifest = {
        "schema_version": "2",
        "name": "acme/signed",
        "version": "1.0.0",
        "kind": "aagent",
        "description": "a signed agent",
        "config": {"kind": "aagent", "system_prompt": "hi"},
        "signature": {
            "alg": "ed25519",
            "key_fingerprint": "abc123",
            "value": "base64sigvalue",
            "scope": "bundle",
        },
    }
    ok, errors = validate(manifest)
    assert ok, f"signature block should be valid: {errors}"


def test_skill_builder_model_field(tmp_path: Path) -> None:
    out = tmp_path / "skill-0.1.0.aagent"
    manifest = (
        Skill("acme/my-skill", "0.1.0")
        .description("a skill")
        .system_prompt("You are specialized.")
        .model("claude-haiku")
        .pack(out)
    )
    assert manifest["kind"] == "aagent"
    assert manifest["config"]["model"] == "claude-haiku"
    assert "tags" not in manifest["config"]
    verify(out)


def test_invalid_manifest_missing_description_raises(tmp_path: Path) -> None:
    out = tmp_path / "broken-0.0.1.aagent"
    bad = (
        Agent("broken", "0.0.1")
        .system_prompt("You are broken.")
    )
    with pytest.raises(ValueError, match="Invalid atool manifest"):
        bad.pack(out)


# --- Migration tests ---

def test_migrate_v1_tool_becomes_mcp_by_default() -> None:
    v1 = {
        "schema_version": "1",
        "name": "acme/mytool",
        "version": "0.1.0",
        "kind": "tool",
        "description": "http tool",
        "config": {"kind": "tool", "binary": "bin/x", "transport": "http"},
    }
    m, _warnings, errors = _migrate_manifest(v1)
    assert not errors
    assert m["kind"] == "mcp"
    assert m["config"]["kind"] == "mcp"


def test_migrate_v1_grpc_tool_becomes_atool() -> None:
    v1 = {
        "schema_version": "1",
        "name": "acme/mytool",
        "version": "0.1.0",
        "kind": "tool",
        "description": "grpc tool",
        "config": {"kind": "tool", "binary": "bin/x", "transport": "grpc"},
    }
    m, _warnings, errors = _migrate_manifest(v1)
    assert not errors
    assert m["kind"] == "atool"
    assert m["config"]["kind"] == "atool"


def test_migrate_v1_agent_keeps_model() -> None:
    v1 = {
        "schema_version": "1",
        "name": "acme/myagent",
        "version": "0.1.0",
        "kind": "agent",
        "description": "test agent",
        "config": {
            "kind": "agent",
            "system_prompt": "hello",
            "model": "claude-opus-4-7",
        },
    }
    m, _warnings, errors = _migrate_manifest(v1)
    assert not errors
    assert m["schema_version"] == "2"
    assert m["kind"] == "aagent"
    assert m["config"]["kind"] == "aagent"
    assert m["config"]["model"] == "claude-opus-4-7"
    assert "llm" not in m["config"]


def test_migrate_intermediate_llm_folds_to_model() -> None:
    v1 = {
        "schema_version": "1",
        "name": "acme/myagent",
        "version": "0.1.0",
        "kind": "agent",
        "description": "test agent",
        "config": {"kind": "agent", "system_prompt": "hello", "llm": "claude-opus-4-7"},
    }
    m, warnings, errors = _migrate_manifest(v1)
    assert not errors
    assert m["config"]["model"] == "claude-opus-4-7"
    assert "llm" not in m["config"]
    assert any("llm renamed to config.model" in w for w in warnings)


def test_migrate_v1_skill_to_aagent_drops_tags() -> None:
    v1 = {
        "schema_version": "1",
        "name": "acme/myskill",
        "version": "0.2.0",
        "kind": "skill",
        "description": "a skill",
        "config": {
            "kind": "skill",
            "system_prompt": "hi",
            "model_hint": "claude-haiku",
            "tags": ["a", "b"],
        },
    }
    m, warnings, errors = _migrate_manifest(v1)
    assert not errors
    assert m["kind"] == "aagent"
    assert m["config"]["kind"] == "aagent"
    assert m["config"]["model"] == "claude-haiku"
    assert "model_hint" not in m["config"]
    assert "tags" not in m["config"]
    assert any("tags removed" in w for w in warnings)


def test_migrate_v1_bundle_to_aagent() -> None:
    v1 = {
        "schema_version": "1",
        "name": "acme/mybundle",
        "version": "0.1.0",
        "kind": "bundle",
        "description": "a bundle",
        "config": {
            "kind": "bundle",
            "components": ["acme/foo@1.0.0", "acme/bar@2.0.0"],
        },
    }
    m, warnings, errors = _migrate_manifest(v1)
    assert not errors
    assert m["kind"] == "aagent"
    assert m["components"] == [{"ref": "acme/foo@1.0.0"}, {"ref": "acme/bar@2.0.0"}]
    assert any("bundle converted" in w for w in warnings)


def test_migrate_llm_runtime_errors() -> None:
    v1 = {
        "schema_version": "1",
        "name": "acme/myruntime",
        "version": "0.1.0",
        "kind": "llm-runtime",
        "description": "a runtime",
        "config": {"kind": "llm-runtime"},
    }
    _m, _warnings, errors = _migrate_manifest(v1)
    assert len(errors) == 1
    assert "llm-runtime" in errors[0]


def test_migrate_llm_backend_errors() -> None:
    v1 = {
        "schema_version": "1",
        "name": "acme/backend",
        "version": "0.1.0",
        "kind": "llm-backend",
        "description": "a backend",
        "config": {"kind": "llm-backend"},
    }
    _m, _warnings, errors = _migrate_manifest(v1)
    assert len(errors) == 1
    assert "llm-backend" in errors[0]
