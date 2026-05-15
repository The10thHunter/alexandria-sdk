"""Command-line interface for the Alexandria Python SDK."""

from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path
from typing import Any

from .pack import inspect, pack, verify

HELP = """alex-sdk — author .atool / .aagent packages

USAGE
  alex-sdk init <template> <dir>     Scaffold a new package source dir
  alex-sdk pack <src-dir> [-o out]   Pack into .atool or .aagent
  alex-sdk verify <pkg>              Re-hash files, validate manifest
  alex-sdk inspect <pkg>             Print manifest + file list
  alex-sdk migrate <src> [-o out]    Upgrade v1 atool.json to v2

TEMPLATES
  tool-node, tool-python, agent-basic, agent-collection

EXAMPLES
  alex-sdk init agent-basic ./my-agent
  alex-sdk pack ./my-agent -o my-agent-0.1.0.aagent
  alex-sdk verify my-agent-0.1.0.aagent
  alex-sdk migrate old-atool.json -o atool.json
"""


def _die(msg: str, code: int = 1) -> None:
    sys.stderr.write(msg + "\n")
    sys.exit(code)


def _templates_root() -> Path:
    """Find the templates/ directory by walking up from this package."""
    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "templates"
        if candidate.is_dir():
            return candidate
    raise FileNotFoundError(
        f"templates/ not found near {here}"
    )


def _default_out_path(src_dir: Path, manifest_kind: str) -> str:
    m = json.loads((src_dir / "atool.json").read_text(encoding="utf-8"))
    short = str(m["name"]).split("/")[-1]
    ext = "aagent" if manifest_kind == "agent" else "atool"
    return f"{short}-{m['version']}.{ext}"


def _migrate_manifest(v1: dict[str, Any]) -> tuple[dict[str, Any], list[str], list[str]]:
    """Migrate a v1 manifest dict to v2. Returns (manifest, warnings, errors)."""
    warnings: list[str] = []
    errors: list[str] = []
    m: dict[str, Any] = dict(v1)

    # Bump schema_version
    m["schema_version"] = "2"

    # Handle removed kinds
    kind = m.get("kind", "")
    if kind in ("llm-runtime", "llm-backend"):
        errors.append(
            f"kind '{kind}' has no v2 equivalent; register via `alexandria llm install` instead"
        )
        return m, warnings, errors

    if kind == "bundle":
        m["kind"] = "agent"
        warnings.append("bundle converted to agent; add config.system_prompt before publishing")
        cfg = dict(m.get("config") or {})
        old_components = cfg.get("components", [])
        if isinstance(old_components, list):
            m["components"] = [{"ref": ref} for ref in old_components]
        m["config"] = {
            "kind": "agent",
            "system_prompt": "TODO: add system_prompt",
        }
        cfg = m["config"]
    else:
        cfg = dict(m.get("config") or {})

    # Migrate config fields
    if "model" in cfg:
        cfg["llm"] = cfg.pop("model")
        warnings.append("config.model renamed to config.llm")
    if "model_hint" in cfg:
        cfg["llm"] = cfg.pop("model_hint")
        warnings.append("config.model_hint renamed to config.llm")
    if "default_mode" in cfg:
        del cfg["default_mode"]
        warnings.append("config.default_mode removed (swarm is always default)")
    m["config"] = cfg

    # Strip old signing fields at wrong locations
    stripped_signing: list[str] = []
    for field in ("signed_at", "key_fingerprint"):
        if field in m:
            del m[field]
            stripped_signing.append(field)
    # If signature present but not in v2 shape, remove it
    if "signature" in m:
        sig = m["signature"]
        has_v2_shape = (
            isinstance(sig, dict)
            and "alg" in sig
            and "key_fingerprint" in sig
            and "value" in sig
            and "scope" in sig
        )
        if not has_v2_shape:
            del m["signature"]
            stripped_signing.append("signature")
    if stripped_signing:
        warnings.append(
            f"signing fields removed ({', '.join(stripped_signing)}); re-sign after migration"
        )

    # Warn about default_port: 0
    if cfg.get("default_port") == 0:
        warnings.append("default_port was 0 (schema-invalid); set to a valid port 1-65535")

    # Warn about dependencies missing version
    for dep in m.get("dependencies") or []:
        if not dep.get("version"):
            warnings.append(
                f"dependency '{dep.get('name', '?')}' missing version field; add before publishing"
            )

    return m, warnings, errors


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if not args or args[0] in ("-h", "--help"):
        sys.stdout.write(HELP)
        return 0

    cmd, *rest = args

    if cmd == "init":
        if len(rest) < 2:
            _die("usage: alex-sdk init <template> <dir>")
        tpl, dest = rest[0], rest[1]
        try:
            root = _templates_root()
        except FileNotFoundError as e:
            _die(str(e))
            return 1
        src = root / tpl
        if not src.is_dir():
            available = ", ".join(sorted(p.name for p in root.iterdir() if p.is_dir()))
            _die(f"unknown template '{tpl}'. Available: {available}")
        dest_path = Path(dest)
        dest_path.mkdir(parents=True, exist_ok=True)
        shutil.copytree(src, dest_path, dirs_exist_ok=True)
        sys.stdout.write(
            f"Scaffolded {tpl} into {dest}\nEdit atool.json, then: alex-sdk pack {dest}\n"
        )
        return 0

    if cmd == "pack":
        if not rest:
            _die("usage: alex-sdk pack <src-dir> [-o out]")
        src_dir = Path(rest[0])
        out: str
        if "-o" in rest:
            i = rest.index("-o")
            if i + 1 >= len(rest):
                _die("usage: alex-sdk pack <src-dir> [-o out]")
            out = rest[i + 1]
        else:
            m = json.loads((src_dir / "atool.json").read_text(encoding="utf-8"))
            out = _default_out_path(src_dir, m["kind"])
        manifest = pack(src_dir, out)
        sys.stdout.write(
            f"Packed {manifest['name']}@{manifest['version']} -> {out}\n"
        )
        return 0

    if cmd == "verify":
        if not rest:
            _die("usage: alex-sdk verify <pkg>")
        m = verify(rest[0])
        sys.stdout.write(f"OK {m['name']}@{m['version']} (kind={m['kind']})\n")
        return 0

    if cmd == "inspect":
        if not rest:
            _die("usage: alex-sdk inspect <pkg>")
        r = inspect(rest[0])
        sys.stdout.write(json.dumps(r.to_dict(), indent=2) + "\n")
        return 0

    if cmd == "migrate":
        if not rest:
            _die("usage: alex-sdk migrate <src> [-o <out>]")
        src_arg = rest[0]
        out_path: str | None = None
        if "-o" in rest:
            i = rest.index("-o")
            if i + 1 < len(rest):
                out_path = rest[i + 1]

        src = Path(src_arg)
        if src.is_dir():
            resolved = src / "atool.json"
        else:
            resolved = src

        try:
            raw = resolved.read_text(encoding="utf-8")
        except OSError as e:
            _die(f"cannot read {resolved}: {e}")
            return 1

        try:
            v1 = json.loads(raw)
        except json.JSONDecodeError as e:
            _die(f"invalid JSON in {resolved}: {e}")
            return 1

        manifest, warnings, errors = _migrate_manifest(v1)

        if errors:
            sys.stderr.write("Migration errors:\n")
            for err in errors:
                sys.stderr.write(f"  ERROR: {err}\n")
            return 1

        dest = Path(out_path) if out_path else resolved
        dest.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")

        if warnings:
            sys.stderr.write("Migration warnings:\n")
            for w in warnings:
                sys.stderr.write(f"  WARN: {w}\n")

        sys.stdout.write(f"Migrated to v2 -> {dest}\n")
        return 0

    _die(f"unknown command '{cmd}'\n\n{HELP}")
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
