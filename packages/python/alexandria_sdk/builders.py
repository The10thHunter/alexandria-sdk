"""Fluent builders for `.atool` / `.aagent` manifests (EE-canonical schema v2).

Kinds mirror ``ee/crates/alex-package/src/manifest.rs``:

- ``mcp``    — binary tool daemon over the MCP protocol (JSON-RPC/SSE)
- ``atool``  — binary tool daemon over the native gRPC ``ToolService``
- ``aagent`` — orchestrator-managed agent (``system_prompt``/``allowed_tools``/``model``).
               A "skill" is reusable prompt text that ships as an aagent whose
               content is its ``system_prompt`` — there is no standalone skill kind.
"""

from __future__ import annotations

import copy
import json
import shutil
import tempfile
from pathlib import Path
from typing import Any

from . import pack as _pack_mod
from .schema import assert_valid


class _Base:
    """Common manifest fields shared by Tool, Agent, Skill."""

    def __init__(self, name: str, version: str, kind: str, config: dict[str, Any]) -> None:
        self._manifest: dict[str, Any] = {
            "schema_version": "2",
            "name": name,
            "version": version,
            "kind": kind,
            "description": "",
            "config": config,
        }
        # archive_path -> absolute source path on disk
        self._staged: list[tuple[str, str]] = []

    # --- common manifest fields ---
    def description(self, d: str) -> "_Base":
        self._manifest["description"] = d
        return self

    def author(self, a: str) -> "_Base":
        self._manifest["author"] = a
        return self

    def license(self, l: str) -> "_Base":
        self._manifest["license"] = l
        return self

    def requires_alexandria(self, v: str) -> "_Base":
        self._manifest["requires_alexandria"] = v
        return self

    def dependency(self, dep: dict[str, str]) -> "_Base":
        self._manifest.setdefault("dependencies", []).append(dep)
        return self

    def dependencies(self, deps: list[dict[str, str]]) -> "_Base":
        self._manifest["dependencies"] = list(deps)
        return self

    def file(self, entry: dict[str, Any]) -> "_Base":
        self._manifest.setdefault("files", []).append(dict(entry))
        return self

    def files(self, entries: list[dict[str, Any]]) -> "_Base":
        self._manifest["files"] = [dict(e) for e in entries]
        return self

    def _ensure_perms(self) -> dict[str, Any]:
        return self._manifest.setdefault("permissions", {})

    def provides_tools(self, t: list[str]) -> "_Base":
        self._ensure_perms()["provides_tools"] = list(t)
        return self

    def needs_tools(self, t: list[str]) -> "_Base":
        self._ensure_perms()["needs_tools"] = list(t)
        return self

    def suggested_role(self, r: str) -> "_Base":
        self._ensure_perms()["suggested_role"] = r
        return self

    def stage_file(
        self,
        src_path: str | Path,
        archive_path: str,
        install_path: str,
        executable: bool = False,
    ) -> "_Base":
        """Stage a file from disk to be included at ``archive_path`` on pack.

        Automatically appends a matching ``files[]`` entry.
        """
        entry: dict[str, Any] = {
            "archive_path": archive_path,
            "install_path": install_path,
        }
        if executable:
            entry["executable"] = True
        self._staged.append((archive_path, str(Path(src_path).resolve())))
        return self.file(entry)

    def build(self) -> dict:
        m = copy.deepcopy(self._manifest)
        return assert_valid(m)

    def pack(self, out_path: str | Path, src_dir: str | Path | None = None) -> dict:
        """Materialize the manifest (plus staged files) and pack to ``out_path``.

        If ``src_dir`` is given, the manifest is written into that directory and
        any declared files are expected to already be present.
        """
        if src_dir is not None:
            sd = Path(src_dir)
            sd.mkdir(parents=True, exist_ok=True)
            m = self.build()
            (sd / "atool.json").write_text(
                json.dumps(m, indent=2) + "\n", encoding="utf-8"
            )
            return _pack_mod.pack(sd, out_path)

        tmp = Path(tempfile.mkdtemp(prefix="alex-sdk-"))
        try:
            m = self.build()
            (tmp / "atool.json").write_text(
                json.dumps(m, indent=2) + "\n", encoding="utf-8"
            )
            for archive_path, src_abs in self._staged:
                dest = tmp / archive_path
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src_abs, dest)
            return _pack_mod.pack(tmp, out_path)
        finally:
            shutil.rmtree(tmp, ignore_errors=True)


class Tool(_Base):
    """Binary tool builder.

    Emits ``kind = atool`` (native gRPC ``ToolService``) by default; calling
    ``.transport("http" | "sse")`` re-taxes the package to ``kind = mcp`` (MCP
    JSON-RPC/SSE). ``.transport("grpc")`` keeps it an atool.
    """

    def __init__(self, name: str, version: str) -> None:
        # Default transport is gRPC => atool, matching EE's AtoolConfig default.
        super().__init__(name, version, "atool", {"kind": "atool", "binary": ""})

    def binary(self, p: str) -> "Tool":
        self._manifest["config"]["binary"] = p
        return self

    def port(self, p: int) -> "Tool":
        self._manifest["config"]["default_port"] = p
        return self

    def transport(self, t: str) -> "Tool":
        """Pick the wire protocol — and thereby the package kind.

        ``"grpc"`` => kind atool (native ToolService); ``"http"``/``"sse"`` =>
        kind mcp (MCP JSON-RPC/SSE).
        """
        kind = "atool" if t == "grpc" else "mcp"
        self._manifest["kind"] = kind
        self._manifest["config"]["kind"] = kind
        self._manifest["config"]["transport"] = t
        return self

    def args(self, a: list[str]) -> "Tool":
        self._manifest["config"]["args"] = list(a)
        return self

    def interface_major(self, n: int) -> "Tool":
        """Contract/ABI major this tool exposes over its wire protocol (EE default 1)."""
        self._manifest["config"]["interface_major"] = n
        return self

    def k8s_image(self, img: str) -> "Tool":
        self._manifest["config"]["k8s_image"] = img
        return self

    def k8s_capabilities(self, c: list[str]) -> "Tool":
        self._manifest["config"]["k8s_capabilities"] = list(c)
        return self

    def k8s_port(self, p: int) -> "Tool":
        self._manifest["config"]["k8s_port"] = p
        return self

    def k8s_transport(self, t: str) -> "Tool":
        self._manifest["config"]["k8s_transport"] = t
        return self

    def k8s_resources(self, r: dict[str, Any]) -> "Tool":
        self._manifest["config"]["k8s_resources"] = dict(r)
        return self

    def k8s_min_warm(self, n: int) -> "Tool":
        self._manifest["config"]["k8s_min_warm"] = n
        return self

    def k8s_idle_timeout(self, seconds: int) -> "Tool":
        self._manifest["config"]["k8s_idle_timeout_seconds"] = seconds
        return self


class Agent(_Base):
    def __init__(self, name: str, version: str) -> None:
        super().__init__(name, version, "aagent", {"kind": "aagent", "system_prompt": ""})

    def system_prompt(self, s: str) -> "Agent":
        self._manifest["config"]["system_prompt"] = s
        return self

    def system_prompt_from_file(self, p: str | Path) -> "Agent":
        self._manifest["config"]["system_prompt"] = Path(p).read_text(encoding="utf-8")
        return self

    def allowed_tools(self, t: list[str]) -> "Agent":
        self._manifest["config"]["allowed_tools"] = list(t)
        return self

    def model(self, m: str) -> "Agent":
        """Preferred model backend id (EE ``config.model``). Replaces v1 ``.llm()``."""
        self._manifest["config"]["model"] = m
        return self

    def history_limit(self, n: int) -> "Agent":
        self._manifest["config"]["history_limit"] = n
        return self

    def prompt_mode(self, m: str) -> "Agent":
        """Prompt composition mode against ``extends`` bases: "append" (default) | "replace"."""
        self._manifest["config"]["prompt_mode"] = m
        return self

    def extend(self, base: dict[str, str]) -> "Agent":
        """Append a base package this aagent extends. aagent-only in EE."""
        self._manifest.setdefault("extends", []).append(dict(base))
        return self

    def extends_packages(self, bases: list[dict[str, str]]) -> "Agent":
        self._manifest["extends"] = [dict(b) for b in bases]
        return self

    def lock(self, entry: dict[str, Any]) -> "Agent":
        """Append a resolved inheritance lockfile entry (aagent-only)."""
        self._manifest.setdefault("lockfile", []).append(dict(entry))
        return self

    def lockfile(self, entries: list[dict[str, Any]]) -> "Agent":
        self._manifest["lockfile"] = [dict(e) for e in entries]
        return self

    def component(self, name: str, id: str, child: "Agent | Skill") -> "Agent":
        """Append an inline sub-agent component.

        ``name`` is the local label; ``id`` is the canonical ns/name@version.
        Both Agent and Skill children emit kind=aagent. Binary tools may only
        appear as refs, never inline.
        """
        child_manifest = child.build()
        item: dict[str, Any] = {
            "name": name,
            "id": id,
            "kind": "aagent",
            "config": child_manifest["config"],
        }
        if child_manifest.get("files"):
            item["files"] = child_manifest["files"]
        if child_manifest.get("permissions"):
            item["permissions"] = child_manifest["permissions"]
        if child_manifest.get("dependencies"):
            item["dependencies"] = child_manifest["dependencies"]
        if child_manifest.get("components"):
            item["components"] = child_manifest["components"]
        self._manifest.setdefault("components", []).append(item)
        return self

    def ref(self, ns_name_at_version: str) -> "Agent":
        """Append an external ref component (any kind: mcp/atool tool, or aagent)."""
        self._manifest.setdefault("components", []).append({"ref": ns_name_at_version})
        return self

    def flatten(self, rules: dict[str, str]) -> "Agent":
        """Set install.flatten merge rules (only meaningful on aagents with components[])."""
        self._manifest.setdefault("install", {})["flatten"] = dict(rules)
        return self


class Skill(_Base):
    """Skill builder.

    A "skill" is reusable prompt text — EE has no standalone skill kind, so this
    emits ``kind = aagent`` whose only content is ``system_prompt``.
    """

    def __init__(self, name: str, version: str) -> None:
        super().__init__(name, version, "aagent", {"kind": "aagent", "system_prompt": ""})

    def system_prompt(self, s: str) -> "Skill":
        self._manifest["config"]["system_prompt"] = s
        return self

    def system_prompt_from_file(self, p: str | Path) -> "Skill":
        self._manifest["config"]["system_prompt"] = Path(p).read_text(encoding="utf-8")
        return self

    def allowed_tools(self, t: list[str]) -> "Skill":
        self._manifest["config"]["allowed_tools"] = list(t)
        return self

    def model(self, m: str) -> "Skill":
        """Preferred model backend id (EE ``config.model``). Replaces v1 ``.model_hint()``."""
        self._manifest["config"]["model"] = m
        return self
