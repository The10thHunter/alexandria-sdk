"""JSON Schema validation against the shared `atool.schema.json`."""

from __future__ import annotations

import json
from dataclasses import dataclass
from functools import lru_cache
from importlib import resources
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator


@dataclass(frozen=True)
class ValidationError:
    path: str
    message: str


def _load_schema_text() -> str:
    # 1) Installed package: schema is shipped as package data.
    try:
        return resources.files(__package__).joinpath("atool.schema.json").read_text(
            encoding="utf-8"
        )
    except (FileNotFoundError, ModuleNotFoundError, AttributeError):
        pass

    # 2) Dev tree: walk up from this file looking for `schemas/atool.schema.json`.
    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "schemas" / "atool.schema.json"
        if candidate.is_file():
            return candidate.read_text(encoding="utf-8")

    raise FileNotFoundError(
        f"atool.schema.json not found near {Path(__file__).resolve()}"
    )


@lru_cache(maxsize=1)
def _validator() -> Draft202012Validator:
    schema = json.loads(_load_schema_text())
    return Draft202012Validator(schema)


def validate(manifest: Any) -> tuple[bool, list[ValidationError]]:
    """Validate a manifest dict against the schema.

    Returns ``(ok, errors)``. When ``ok`` is True, ``errors`` is empty.
    """
    v = _validator()
    errors = sorted(v.iter_errors(manifest), key=lambda e: list(e.absolute_path))
    if not errors:
        return True, []
    out: list[ValidationError] = []
    for e in errors:
        path = "/" + "/".join(str(p) for p in e.absolute_path) if e.absolute_path else "(root)"
        out.append(ValidationError(path=path, message=e.message))
    return False, out


def assert_valid(manifest: Any) -> dict:
    """Raise ``ValueError`` with a formatted message if invalid; else return the manifest."""
    ok, errors = validate(manifest)
    if ok:
        return manifest
    formatted = "\n".join(f"  {e.path}: {e.message}" for e in errors)
    raise ValueError(f"Invalid atool manifest:\n{formatted}")
