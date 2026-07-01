"""Tests for the Alexandria Python SDK publish() flow.

The HTTP call is mocked via the injectable ``transport`` seam so no live
registry is required — mirrors the TypeScript SDK's ``test/publish.test.ts``.
"""

from __future__ import annotations

from pathlib import Path

from alexandria_sdk import Agent
from alexandria_sdk.publish import publish


def _fixture(tmp_path: Path) -> Path:
    """Build a real packed .aagent so publish()'s verify() step passes."""
    out = tmp_path / "doer-0.1.0.aagent"
    (
        Agent("essentials/doer", "0.1.0")
        .description("doer")
        .system_prompt("You are the doer.")
        .model("claude-opus-4-7")
        .pack(out)
    )
    return out


def test_publish_derives_artifact_type_and_posts_tarball(tmp_path: Path) -> None:
    out = _fixture(tmp_path)
    captured: dict = {}

    def transport(url: str, body: bytes, headers: dict[str, str]) -> tuple[int, str]:
        captured["url"] = url
        captured["body"] = body
        captured["headers"] = headers
        return 202, '{"assessment_id": "abc"}'

    r = publish(out, "https://reg.example/", token="sekret", transport=transport)

    assert r.ok is True
    assert r.status == 202
    assert r.artifact_type == "aagent"
    assert r.body == {"assessment_id": "abc"}
    # trailing slash trimmed
    assert captured["url"] == "https://reg.example/v1/submit"
    assert captured["headers"]["Authorization"] == "Bearer sekret"
    # multipart body carries artifact_type=aagent (derived from kind) + tarball part
    assert b'name="artifact_type"' in captured["body"]
    assert b"aagent" in captured["body"]
    assert b'name="tarball"' in captured["body"]
    assert b'filename="doer-0.1.0.aagent"' in captured["body"]


def test_publish_surfaces_non_2xx_as_not_ok(tmp_path: Path) -> None:
    out = _fixture(tmp_path)

    def transport(url: str, body: bytes, headers: dict[str, str]) -> tuple[int, str]:
        return 400, '{"error": "stage1_kind_enum"}'

    r = publish(out, "https://reg.example", transport=transport)
    assert r.ok is False
    assert r.status == 400


def test_publish_honors_artifact_type_override(tmp_path: Path) -> None:
    out = _fixture(tmp_path)
    captured: dict = {}

    def transport(url: str, body: bytes, headers: dict[str, str]) -> tuple[int, str]:
        captured["body"] = body
        return 202, "{}"

    r = publish(out, "https://reg.example", artifact_type="amodel:llm-backend", transport=transport)
    assert r.artifact_type == "amodel:llm-backend"
    assert b"amodel:llm-backend" in captured["body"]
