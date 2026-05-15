//! Pack / verify / inspect for `.atool` and `.aagent` archives.
//!
//! Wire format (must match `alex_package` in the Alexandria monorepo and the
//! TypeScript SDK):
//!
//! 1. gzipped tar
//! 2. `atool.json` is the FIRST entry, with the fully-populated manifest
//!    (every `files[]` entry's `sha256` filled in)
//! 3. every other entry follows in `files[]` declaration order
//! 4. mode is `0o755` if the entry is `executable`, else `0o644`

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use flate2::Compression;
use sha2::{Digest, Sha256};

use crate::manifest::Manifest;
use crate::schema;
use crate::{Error, Result};

const MANIFEST_NAME: &str = "atool.json";

fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Pack `src_dir` into `out_path`, populating sha256 hashes in-place.
///
/// Returns the manifest that was actually written to the archive (with sha256s
/// filled in).
pub fn pack(src_dir: &Path, out_path: &Path) -> Result<Manifest> {
    let manifest_path = src_dir.join(MANIFEST_NAME);
    let raw = std::fs::read(&manifest_path)?;
    let mut manifest: Manifest = serde_json::from_slice(&raw)?;

    // Step 1: compute sha256 for every declared file, in order.
    if let Some(files) = manifest.files.as_mut() {
        for f in files.iter_mut() {
            let abs = src_dir.join(&f.archive_path);
            f.sha256 = Some(sha256_file(&abs)?);
        }
    }

    // Step 2: validate the now-hashed manifest before serialising it.
    let manifest_value = serde_json::to_value(&manifest)?;
    schema::assert_valid(&manifest_value)?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;

    // Step 3: emit gzipped tar — atool.json first, then files in order.
    let out_file = File::create(out_path)?;
    let gz = GzEncoder::new(out_file, Compression::default());
    let mut builder = tar::Builder::new(gz);

    {
        let mut header = tar::Header::new_gnu();
        header.set_path(MANIFEST_NAME)?;
        header.set_size(manifest_bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, MANIFEST_NAME, manifest_bytes.as_slice())?;
    }

    if let Some(files) = manifest.files.as_ref() {
        for f in files {
            let abs = src_dir.join(&f.archive_path);
            let meta = std::fs::metadata(&abs)?;
            let mode: u32 = if f.executable.unwrap_or(false) { 0o755 } else { 0o644 };
            let mut header = tar::Header::new_gnu();
            header.set_path(&f.archive_path)?;
            header.set_size(meta.len());
            header.set_mode(mode);
            header.set_cksum();
            let file = File::open(&abs)?;
            builder.append(&header, file)?;
        }
    }

    let gz = builder.into_inner()?;
    gz.finish()?;

    Ok(manifest)
}

/// Verify a `.atool`/`.aagent` package: re-hashes every declared file with a
/// non-empty `sha256` and bubbles a mismatch as [`Error::Sha256Mismatch`].
pub fn verify(pkg_path: &Path) -> Result<Manifest> {
    let archive = read_archive(pkg_path, /* keep_bytes = */ true)?;
    let manifest = archive.manifest;
    let value = serde_json::to_value(&manifest)?;
    schema::assert_valid(&value)?;

    if let Some(files) = manifest.files.as_ref() {
        for f in files {
            let want = match f.sha256.as_deref() {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            };
            let bytes = archive
                .file_bytes
                .iter()
                .find(|(name, _)| name == &f.archive_path)
                .map(|(_, b)| b)
                .ok_or_else(|| Error::MissingFile(f.archive_path.clone()))?;
            let got = sha256_bytes(bytes);
            if got != want {
                return Err(Error::Sha256Mismatch {
                    path: f.archive_path.clone(),
                    want: want.to_string(),
                    got,
                });
            }
        }
    }

    Ok(manifest)
}

/// Per-entry summary returned from [`inspect`].
#[derive(Debug, Clone)]
pub struct InspectedFile {
    pub name: String,
    pub size: u64,
}

/// Lightweight peek at an archive's contents.
#[derive(Debug, Clone)]
pub struct InspectResult {
    pub manifest: Manifest,
    pub files: Vec<InspectedFile>,
    pub total_bytes: u64,
}

pub fn inspect(pkg_path: &Path) -> Result<InspectResult> {
    let archive = read_archive(pkg_path, /* keep_bytes = */ false)?;
    let mut total = 0u64;
    let files: Vec<InspectedFile> = archive
        .sizes
        .into_iter()
        .map(|(name, size)| {
            total += size;
            InspectedFile { name, size }
        })
        .collect();
    Ok(InspectResult {
        manifest: archive.manifest,
        files,
        total_bytes: total,
    })
}

struct ReadArchive {
    manifest: Manifest,
    /// `(archive_path, bytes)`. Only populated when `keep_bytes` is true.
    file_bytes: Vec<(String, Vec<u8>)>,
    /// `(archive_path, size_in_bytes)` for every entry.
    sizes: Vec<(String, u64)>,
}

fn read_archive(pkg_path: &Path, keep_bytes: bool) -> Result<ReadArchive> {
    let f = File::open(pkg_path)?;
    let gz = GzDecoder::new(f);
    let mut archive = tar::Archive::new(gz);

    let mut manifest: Option<Manifest> = None;
    let mut file_bytes: Vec<(String, Vec<u8>)> = Vec::new();
    let mut sizes: Vec<(String, u64)> = Vec::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        sizes.push((path.clone(), buf.len() as u64));
        if path == MANIFEST_NAME {
            manifest = Some(serde_json::from_slice(&buf)?);
        } else if keep_bytes {
            file_bytes.push((path, buf));
        }
    }

    let manifest = manifest.ok_or(Error::MissingManifest)?;
    Ok(ReadArchive {
        manifest,
        file_bytes,
        sizes,
    })
}

/// Convenience: write a manifest to `<src_dir>/atool.json`.
///
/// Mirrors the TypeScript `writeManifest` helper; used by the builder when
/// materialising a tempdir before calling [`pack`].
pub(crate) fn write_manifest(src_dir: &Path, manifest: &Manifest) -> Result<()> {
    let path = src_dir.join(MANIFEST_NAME);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = File::create(&path)?;
    let mut buf = serde_json::to_vec_pretty(manifest)?;
    buf.push(b'\n');
    f.write_all(&buf)?;
    Ok(())
}

/// Convenience: load a manifest from `<src_dir>/atool.json`.
pub fn read_manifest(src_dir: &Path) -> Result<Manifest> {
    let path = src_dir.join(MANIFEST_NAME);
    let bytes = std::fs::read(&path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[allow(dead_code)]
pub(crate) fn staged_dest(dir: &Path, archive_path: &str) -> PathBuf {
    dir.join(archive_path)
}
