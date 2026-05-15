"""Pack / verify / inspect `.atool` and `.aagent` archives.

Format: gzipped tar with `atool.json` at the root, written first, followed by
each file declared in ``manifest.files[]`` in declaration order.
"""

from __future__ import annotations

import hashlib
import io
import json
import tarfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .schema import assert_valid


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def _add_bytes(tar: tarfile.TarFile, name: str, data: bytes, mode: int) -> None:
    info = tarfile.TarInfo(name=name)
    info.size = len(data)
    info.mode = mode
    tar.addfile(info, io.BytesIO(data))


def _add_file(tar: tarfile.TarFile, name: str, src: Path, mode: int) -> None:
    info = tarfile.TarInfo(name=name)
    info.size = src.stat().st_size
    info.mode = mode
    with src.open("rb") as f:
        tar.addfile(info, f)


def pack(src_dir: str | Path, out_path: str | Path) -> dict:
    """Pack ``src_dir`` (which must contain ``atool.json``) into ``out_path``.

    For every entry in ``files[]``, the file at ``src_dir/archive_path`` is
    hashed and the hash written into ``sha256`` before the manifest is
    serialized. Returns the final manifest dict.
    """
    src = Path(src_dir).resolve()
    out = Path(out_path)
    manifest_path = src / "atool.json"
    manifest: dict[str, Any] = json.loads(manifest_path.read_text(encoding="utf-8"))

    for entry in manifest.get("files") or []:
        abs_path = (src / entry["archive_path"]).resolve()
        entry["sha256"] = _sha256_file(abs_path)

    assert_valid(manifest)

    manifest_bytes = json.dumps(manifest, indent=2).encode("utf-8")

    with tarfile.open(out, mode="w:gz") as tar:
        _add_bytes(tar, "atool.json", manifest_bytes, 0o644)
        for entry in manifest.get("files") or []:
            abs_path = (src / entry["archive_path"]).resolve()
            mode = 0o755 if entry.get("executable") else 0o644
            _add_file(tar, entry["archive_path"], abs_path, mode)

    return manifest


def _read_archive(pkg_path: str | Path) -> tuple[dict, dict[str, bytes]]:
    files: dict[str, bytes] = {}
    manifest: dict | None = None
    with tarfile.open(pkg_path, mode="r:gz") as tar:
        for member in tar:
            if not member.isfile():
                continue
            f = tar.extractfile(member)
            if f is None:
                continue
            data = f.read()
            if member.name == "atool.json":
                manifest = json.loads(data.decode("utf-8"))
            else:
                files[member.name] = data
    if manifest is None:
        raise ValueError("atool.json not found in archive")
    return manifest, files


def verify(pkg_path: str | Path) -> dict:
    """Extract ``pkg_path``, validate the manifest, and re-hash every declared
    file with a non-empty ``sha256``. Raises on any mismatch."""
    manifest, files = _read_archive(pkg_path)
    assert_valid(manifest)

    for entry in manifest.get("files") or []:
        want = entry.get("sha256")
        if not want:
            continue
        name = entry["archive_path"]
        data = files.get(name)
        if data is None:
            raise ValueError(f"declared file missing from archive: {name}")
        got = hashlib.sha256(data).hexdigest()
        if got != want:
            raise ValueError(
                f"sha256 mismatch for {name}: want {want}, got {got}"
            )
    return manifest


@dataclass
class InspectResult:
    manifest: dict
    files: list[dict]
    total_bytes: int

    def to_dict(self) -> dict:
        return {
            "manifest": self.manifest,
            "files": self.files,
            "totalBytes": self.total_bytes,
        }


def inspect(pkg_path: str | Path) -> InspectResult:
    """Read manifest plus a name/size listing of every archive entry."""
    manifest, files = _read_archive(pkg_path)
    listing = [{"name": n, "size": len(b)} for n, b in files.items()]
    # Include atool.json size for parity with the TS InspectResult.
    manifest_bytes = json.dumps(manifest, indent=2).encode("utf-8")
    listing.insert(0, {"name": "atool.json", "size": len(manifest_bytes)})
    total = sum(item["size"] for item in listing)
    return InspectResult(manifest=manifest, files=listing, total_bytes=total)
