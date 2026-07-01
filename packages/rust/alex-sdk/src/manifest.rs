//! Serde types mirroring the TypeScript `Manifest` interface 1:1
//! (EE-canonical schema v2).
//!
//! Kinds mirror `ee/crates/alex-package/src/manifest.rs`:
//!   `mcp`    — binary tool daemon over the MCP protocol (JSON-RPC/SSE)
//!   `atool`  — binary tool daemon over the native gRPC `ToolService`
//!   `aagent` — orchestrator-managed agent. A "skill" is reusable prompt text
//!              that ships as an aagent whose content is its `system_prompt` —
//!              there is no standalone skill kind.
//!
//! Field names match the on-disk JSON exactly (snake_case). Optional fields use
//! `Option<_>` with `#[serde(skip_serializing_if = "Option::is_none")]` so
//! manifests round-trip cleanly through `serde_json::to_string_pretty`.

use serde::{Deserialize, Serialize};

/// Top-level package manifest. The JSON written into `atool.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: String,
    pub name: String,
    pub version: String,
    pub kind: Kind,
    pub description: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_alexandria: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<Dependency>>,

    /// Base packages this aagent extends. aagent-only — rejected on mcp/atool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<Vec<PackageDep>>,
    /// Resolved inheritance lockfile (aagent-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lockfile: Option<Vec<LockEntry>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,

    pub config: PackageConfig,

    /// Only valid on kind=aagent. Inline sub-components or external refs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<ComponentItem>>,

    /// Install-time merge rules. Only on aagents with non-empty components.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallBlock>,

    /// Signature block (EE-signed archives).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignatureBlock>,
}

/// Package kind. Mirrors the schema enum (lowercase on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Mcp,
    Atool,
    Aagent,
}

/// A single file declared in `files[]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub archive_path: String,
    pub install_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// Optional declarative permissions block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provides_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_role: Option<String>,
}

/// A single declared dependency. `version` is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version: String,
}

/// Base-package reference used by aagent `extends`. Mirrors EE `PackageDep`
/// (`version` has a serde default, so it is optional here).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageDep {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// One resolved entry in an aagent's inheritance `lockfile`. Mirrors EE
/// `LockEntry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockEntry {
    pub name: String,
    pub interface_major: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_hash: Option<String>,
}

/// CPU/memory request spec for the optional `k8s_resources` block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct K8sResourceSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct K8sResources {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests: Option<K8sResourceSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<K8sResourceSpec>,
}

/// The wire protocol a binary tool speaks. Drives the package kind:
/// `grpc` => atool (native ToolService); `http`/`sse` => mcp (MCP JSON-RPC/SSE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireTransport {
    Grpc,
    Http,
    Sse,
}

/// Transports a tool can speak when run under Kubernetes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolK8sTransport {
    Grpc,
    Http,
    Sse,
}

/// k8s Helm-tier hints shared by both binary-tool configs (mcp + atool).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct K8sHints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k8s_image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k8s_capabilities: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k8s_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k8s_transport: Option<ToolK8sTransport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k8s_resources: Option<K8sResources>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k8s_min_warm: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k8s_idle_timeout_seconds: Option<u32>,
}

/// `kind = mcp` config — a binary daemon reached over the MCP protocol.
/// `transport` is the MCP wire (http/sse).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    pub binary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<McpTransport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Contract/ABI major this tool exposes. Defaults to 1 in EE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface_major: Option<u32>,
    #[serde(flatten)]
    pub k8s: K8sHints,
}

/// MCP wire transports (http/sse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    Http,
    Sse,
}

/// `kind = atool` config — a binary daemon reached over the native gRPC
/// `ToolService`. `transport` defaults to "grpc".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtoolConfig {
    pub binary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<WireTransport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Contract/ABI major this tool exposes. Defaults to 1 in EE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface_major: Option<u32>,
    #[serde(flatten)]
    pub k8s: K8sHints,
}

/// `kind = aagent` config. A skill collapses into this shape with only
/// `system_prompt` populated — there is no `tags` field and no standalone
/// skill kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AagentConfig {
    pub system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    /// Preferred model backend id (EE `model`). Replaces v1 `model`/`model_hint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_limit: Option<u32>,
    /// How this prompt composes with `extends` bases: "append" (default) | "replace".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_mode: Option<PromptMode>,
}

/// Prompt composition mode against `extends` bases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptMode {
    Append,
    Replace,
}

/// Merge rules for flattening components into the root aagent at install time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallFlatten {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_limit: Option<String>,
}

/// Install block — merge rules for an aagent with components.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallBlock {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flatten: Option<InstallFlatten>,
}

/// Signature block (EE-signed archives).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureBlock {
    pub alg: String,
    pub key_fingerprint: String,
    pub value: String,
    pub scope: String,
}

/// A discriminated union for components[]: either an external ref or an
/// inline sub-agent.
///
/// On the wire: if `ref` is present → external ref. Otherwise the inline fields apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComponentItem {
    /// External reference: `{ "ref": "ns/name@version" }`.
    Ref(RefComponent),
    /// Inline sub-agent.
    Inline(InlineComponent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefComponent {
    #[serde(rename = "ref")]
    pub ref_target: String,
}

/// Inline sub-agent embedded in a parent aagent's components[]. Binary tools
/// may only appear as refs, never inline — so the inline kind is always aagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineComponent {
    pub name: String,
    /// Canonical ns/name@version for coalescing.
    pub id: String,
    pub kind: InlineComponentKind,
    pub config: InlineConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<ComponentItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<Dependency>>,
}

/// Only aagent may appear inline; binary tools are refs only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InlineComponentKind {
    Aagent,
}

/// Config for an inline component (aagent only).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum InlineConfig {
    Aagent(AagentConfig),
}

/// The discriminated `config` payload. Internally-tagged so the JSON looks like
/// `{"kind":"atool", "binary":...}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PackageConfig {
    Mcp(McpConfig),
    Atool(AtoolConfig),
    Aagent(AagentConfig),
}
