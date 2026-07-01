"""Publish a packed `.atool` / `.aagent` archive to an Alexandria registry.

Mirrors the TypeScript SDK ``publish()``: re-verifies the archive (hashes +
schema) then POSTs a multipart body to ``{registry}/v1/submit`` — the missing
consumer-side half of the registry loop (``alexandria install`` pulls; nothing
pushed until now).

The multipart body mirrors the registry's ``handleSubmit`` contract exactly:

- ``artifact_type`` — derived from the manifest kind (mcp|atool|aagent),
  overridable for amodel sub-variants.
- ``tarball`` — the packed archive bytes.
"""

from __future__ import annotations

import json
import mimetypes
import os
import urllib.error
import urllib.request
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

from .pack import verify

# Injectable transport seam so tests can mock the HTTP call without a live
# server. A transport takes (url, body, headers) and returns (status, text).
Transport = Callable[[str, bytes, dict[str, str]], "tuple[int, str]"]


@dataclass
class PublishResult:
    status: int
    ok: bool
    body: Any
    name: str
    version: str
    artifact_type: str


def _build_multipart(artifact_type: str, filename: str, tarball: bytes) -> tuple[bytes, str]:
    """Assemble a multipart/form-data body with an ``artifact_type`` field and a
    ``tarball`` file part. Returns (body, content_type)."""
    boundary = f"----alexsdk{uuid.uuid4().hex}"
    ctype = mimetypes.guess_type(filename)[0] or "application/gzip"
    parts: list[bytes] = []

    parts.append(f"--{boundary}\r\n".encode())
    parts.append(b'Content-Disposition: form-data; name="artifact_type"\r\n\r\n')
    parts.append(artifact_type.encode() + b"\r\n")

    parts.append(f"--{boundary}\r\n".encode())
    parts.append(
        f'Content-Disposition: form-data; name="tarball"; filename="{filename}"\r\n'.encode()
    )
    parts.append(f"Content-Type: {ctype}\r\n\r\n".encode())
    parts.append(tarball)
    parts.append(b"\r\n")

    parts.append(f"--{boundary}--\r\n".encode())
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


def _default_transport(url: str, body: bytes, headers: dict[str, str]) -> tuple[int, str]:
    req = urllib.request.Request(url, data=body, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req) as resp:  # noqa: S310 (trusted registry URL)
            return resp.status, resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", errors="replace")


def publish(
    pkg_path: str | Path,
    registry: str,
    token: str | None = None,
    artifact_type: str | None = None,
    *,
    transport: Transport | None = None,
) -> PublishResult:
    """Publish ``pkg_path`` to ``{registry}/v1/submit``.

    The archive is re-verified before it ships — never publish an archive the
    local runtime would itself reject. Pass ``transport`` to inject a custom
    HTTP client (used by tests to mock the call).
    """
    manifest = verify(pkg_path)
    resolved_type = artifact_type or str(manifest["kind"])

    tarball = Path(pkg_path).read_bytes()
    body, content_type = _build_multipart(resolved_type, os.path.basename(str(pkg_path)), tarball)

    base = registry.rstrip("/")
    url = f"{base}/v1/submit"
    headers: dict[str, str] = {
        "Content-Type": content_type,
        "Content-Length": str(len(body)),
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"

    send = transport or _default_transport
    status, text = send(url, body, headers)

    try:
        parsed: Any = json.loads(text)
    except (json.JSONDecodeError, ValueError):
        parsed = text

    return PublishResult(
        status=status,
        ok=200 <= status < 300,
        body=parsed,
        name=str(manifest["name"]),
        version=str(manifest["version"]),
        artifact_type=resolved_type,
    )
