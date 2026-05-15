//! Serde types mirroring the TypeScript `Manifest` interface 1:1.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,

    pub config: PackageConfig,

    /// Only valid on kind=agent. Inline sub-components or external refs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<ComponentItem>>,

    /// Install-time merge rules. Only on agents with non-empty components.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallBlock>,

    /// Signature block (EE-signed archives).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignatureBlock>,
}

/// Package kind. Mirrors the schema enum (kebab-case on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Tool,
    Agent,
    Skill,
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

/// A single declared dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version: String,
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

/// Transports a local tool can speak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolTransport {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub binary: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<ToolTransport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    /// Preferred LLM id (freeform, preference only). Replaces v1 `model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfig {
    pub system_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    /// Preferred LLM id (freeform, preference only). Replaces v1 `model_hint`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Merge rules for flattening components into the root agent at install time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstallFlatten {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_limit: Option<String>,
}

/// Install block — merge rules for an agent with components.
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
/// inline sub-agent/sub-skill.
///
/// On the wire: if `ref` is present → external ref. Otherwise the inline fields apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComponentItem {
    /// External reference: `{ "ref": "ns/name@version" }`.
    Ref(RefComponent),
    /// Inline sub-agent or sub-skill.
    Inline(InlineComponent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefComponent {
    #[serde(rename = "ref")]
    pub ref_target: String,
}

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

/// Only agent and skill may appear inline; tools are refs only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InlineComponentKind {
    Agent,
    Skill,
}

/// Config for an inline component (agent or skill only).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InlineConfig {
    Agent(AgentConfig),
    Skill(SkillConfig),
}

/// The discriminated `config` payload. We use serde's internally-tagged enum
/// representation so the JSON looks like `{"kind":"tool", "binary":...}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PackageConfig {
    Tool(ToolConfig),
    Agent(AgentConfig),
    Skill(SkillConfig),
}
