//! `alex-sdk` — author `.atool` / `.aagent` packages.
//!
//! Subcommands mirror the TypeScript CLI: `init`, `pack`, `verify`, `inspect`, `migrate`.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};

use alex_sdk::manifest::Kind;
use alex_sdk::migrate::migrate_manifest;
use alex_sdk::pack::read_manifest;
use alex_sdk::{inspect, pack, verify};

#[derive(Parser, Debug)]
#[command(
    name = "alex-sdk",
    about = "Author .atool / .aagent packages",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Scaffold a new package source dir from a built-in template.
    Init {
        /// Template name: tool-node, tool-python, agent-basic, agent-collection.
        template: String,
        /// Destination directory.
        dir: PathBuf,
    },
    /// Pack a source directory into a `.atool` or `.aagent`.
    Pack {
        /// Source directory containing `atool.json`.
        src_dir: PathBuf,
        /// Output path. Defaults to `<name>-<version>.{atool|aagent}`.
        #[arg(short = 'o', long)]
        out: Option<PathBuf>,
    },
    /// Verify a `.atool` / `.aagent`: re-hash files and validate the manifest.
    Verify { pkg: PathBuf },
    /// Print a package's manifest and entry listing.
    Inspect { pkg: PathBuf },
    /// Upgrade a v1 `atool.json` to v2. Reads a file or directory.
    Migrate {
        /// Source: a v1 `atool.json` file or a directory containing one.
        src: PathBuf,
        /// Output path. Defaults to overwriting the source file.
        #[arg(short = 'o', long)]
        out: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init { template, dir } => cmd_init(&template, &dir),
        Cmd::Pack { src_dir, out } => cmd_pack(&src_dir, out.as_deref()),
        Cmd::Verify { pkg } => cmd_verify(&pkg),
        Cmd::Inspect { pkg } => cmd_inspect(&pkg),
        Cmd::Migrate { src, out } => cmd_migrate(&src, out.as_deref()),
    }
}

/// Walk up from the current exe (and cwd) looking for a `templates/`
/// directory. Mirrors the TS CLI's candidate-list approach.
fn templates_root() -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("templates"));
            let mut up = dir.to_path_buf();
            for _ in 0..6 {
                if let Some(parent) = up.parent() {
                    up = parent.to_path_buf();
                    candidates.push(up.join("templates"));
                } else {
                    break;
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("templates"));
        candidates.push(cwd.join("../templates"));
        candidates.push(cwd.join("../../templates"));
    }
    for c in &candidates {
        if c.is_dir() {
            return Ok(c.clone());
        }
    }
    Err(anyhow!(
        "templates/ not found near the executable.\n\
         Expected one of:\n  {}\n\
         (Looking for `../../templates` relative to the repo root.)",
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n  ")
    ))
}

fn cmd_init(template: &str, dir: &Path) -> Result<()> {
    let root = templates_root()?;
    let src = root.join(template);
    if !src.is_dir() {
        let available: Vec<String> = std::fs::read_dir(&root)
            .with_context(|| format!("reading {}", root.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        bail!(
            "unknown template '{}'. Available: {}",
            template,
            available.join(", ")
        );
    }
    std::fs::create_dir_all(dir)?;
    copy_dir_recursive(&src, dir)?;
    println!(
        "Scaffolded {} into {}\nEdit atool.json, then: alex-sdk pack {}",
        template,
        dir.display(),
        dir.display()
    );
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn default_out_path(src_dir: &Path) -> Result<PathBuf> {
    let manifest = read_manifest(src_dir)
        .map_err(|e| anyhow!("reading {}/atool.json: {e}", src_dir.display()))?;
    let ext = if matches!(manifest.kind, Kind::Agent) {
        "aagent"
    } else {
        "atool"
    };
    let short = manifest.name.rsplit('/').next().unwrap_or(&manifest.name);
    Ok(PathBuf::from(format!(
        "{}-{}.{}",
        short, manifest.version, ext
    )))
}

fn cmd_pack(src_dir: &Path, out: Option<&Path>) -> Result<()> {
    let out_path = match out {
        Some(p) => p.to_path_buf(),
        None => default_out_path(src_dir)?,
    };
    let manifest = pack(src_dir, &out_path).map_err(|e| anyhow!("pack failed: {e}"))?;
    println!(
        "Packed {}@{} -> {}",
        manifest.name,
        manifest.version,
        out_path.display()
    );
    Ok(())
}

fn cmd_verify(pkg: &Path) -> Result<()> {
    let m = verify(pkg).map_err(|e| anyhow!("verify failed: {e}"))?;
    let kind = serde_json::to_value(&m.kind)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "?".to_string());
    println!("OK {}@{} (kind={})", m.name, m.version, kind);
    Ok(())
}

fn cmd_inspect(pkg: &Path) -> Result<()> {
    let r = inspect(pkg).map_err(|e| anyhow!("inspect failed: {e}"))?;
    let files: Vec<serde_json::Value> = r
        .files
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.name,
                "size": f.size,
            })
        })
        .collect();
    let out = serde_json::json!({
        "manifest": r.manifest,
        "files": files,
        "totalBytes": r.total_bytes,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn cmd_migrate(src: &Path, out: Option<&Path>) -> Result<()> {
    // If src is a directory, look for atool.json inside
    let resolved = if src.is_dir() {
        src.join("atool.json")
    } else {
        src.to_path_buf()
    };

    let raw = std::fs::read_to_string(&resolved)
        .with_context(|| format!("reading {}", resolved.display()))?;

    let v1: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parsing JSON in {}", resolved.display()))?;

    let result = migrate_manifest(v1);

    if !result.errors.is_empty() {
        eprintln!("Migration errors:");
        for e in &result.errors {
            eprintln!("  ERROR: {}", e);
        }
        bail!("migration failed: {} error(s)", result.errors.len());
    }

    let json = serde_json::to_string_pretty(&result.manifest)? + "\n";
    let dest = out.unwrap_or(&resolved);
    std::fs::write(dest, &json)
        .with_context(|| format!("writing {}", dest.display()))?;

    if !result.warnings.is_empty() {
        eprintln!("Migration warnings:");
        for w in &result.warnings {
            eprintln!("  WARN: {}", w);
        }
    }

    println!("Migrated to v2 -> {}", dest.display());
    Ok(())
}
