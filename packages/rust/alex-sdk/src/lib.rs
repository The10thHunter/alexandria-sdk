//! Alex SDK — author and verify Alexandria `.atool` / `.aagent` packages.
//!
//! This crate is intentionally standalone: it reimplements the wire format
//! (gzipped tar with `atool.json` at the root, files in declared order, SHA-256
//! pinned per entry) so external authors can use it without depending on the
//! Alexandria monorepo's internal `alex-package` crate.
//!
//! The public surface mirrors the TypeScript SDK:
//!
//! - [`Manifest`] and friends in [`manifest`]
//! - JSON Schema [`validate`] / [`assert_valid`] in [`schema`]
//! - Pack/verify/inspect in [`pack`]
//! - Fluent builders ([`Tool`], [`Agent`], [`Skill`]) in [`builders`]
//! - Manifest migration from v1 to v2 via [`migrate`]

pub mod builders;
pub mod manifest;
pub mod migrate;
pub mod pack;
pub mod schema;

pub use builders::{Agent, Skill, Tool};
pub use manifest::{
    AgentConfig, ComponentItem, Dependency, FileEntry, InlineComponent, InlineComponentKind,
    InlineConfig, InstallBlock, InstallFlatten, K8sResources, K8sResourceSpec, Kind, Manifest,
    PackageConfig, Permissions, RefComponent, SignatureBlock, SkillConfig, ToolConfig,
    ToolK8sTransport, ToolTransport,
};
pub use migrate::{migrate_manifest, MigrateResult};
pub use pack::{inspect, pack, verify, InspectResult, InspectedFile};
pub use schema::{assert_valid, validate, ValidationError};

use thiserror::Error;

/// Errors surfaced by the SDK.
#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid manifest:\n{0}")]
    Schema(String),
    #[error("sha256 mismatch for {path}: want {want}, got {got}")]
    Sha256Mismatch {
        path: String,
        want: String,
        got: String,
    },
    #[error("declared file missing from archive: {0}")]
    MissingFile(String),
    #[error("atool.json not found in archive")]
    MissingManifest,
    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Error::Other(value.to_string())
    }
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
