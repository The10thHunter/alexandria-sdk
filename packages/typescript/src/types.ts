// EE-canonical .atool taxonomy (schema_version "2").
//
// Kinds mirror ee/crates/alex-package/src/manifest.rs::PackageKind exactly:
//   mcp    — binary tool daemon spoken to over the MCP protocol (JSON-RPC/SSE)
//   atool  — binary tool daemon spoken to over the native gRPC ToolService
//   aagent — orchestrator-managed agent (carries system_prompt/allowed_tools/model).
//            "Skills" have no standalone kind — a skill is reusable prompt text
//            that ships as an aagent whose content is its system_prompt.
//
// Field names below are the serde names in manifest.rs; SDK output must
// deserialize into the EE structs unchanged.
export type Kind = "mcp" | "atool" | "aagent";

export interface FileEntry {
  archive_path: string;
  install_path: string;
  executable?: boolean;
  sha256?: string;
}

export interface Permissions {
  provides_tools?: string[];
  needs_tools?: string[];
  suggested_role?: string;
}

/** Dependency on another published .atool package. `version` is required. */
export interface Dependency {
  name: string;
  version: string;
}

/**
 * Base-package reference used by aagent `extends`. Mirrors EE `PackageDep`
 * (`version` has a serde default, so it is optional here).
 */
export interface PackageDep {
  name: string;
  version?: string;
}

/**
 * One resolved entry in an aagent's inheritance `lockfile`. Mirrors EE
 * `LockEntry`: pins the contract/ABI major (and best-effort content hash) of a
 * referenced binary tool at flatten time.
 */
export interface LockEntry {
  name: string;
  interface_major: number;
  contract_hash?: string;
}

export interface K8sResources {
  requests?: { cpu?: string; memory?: string };
  limits?: { cpu?: string; memory?: string };
}

/** k8s Helm-tier hints shared by both binary-tool configs (mcp + atool). */
interface K8sHints {
  k8s_image?: string;
  k8s_capabilities?: string[];
  k8s_port?: number;
  k8s_transport?: "grpc" | "http" | "sse";
  k8s_resources?: K8sResources;
  k8s_min_warm?: number;
  k8s_idle_timeout_seconds?: number;
}

/**
 * `kind = mcp` config — a binary daemon reached over the MCP protocol.
 * Mirrors EE `McpConfig`; `transport` is the MCP wire (http/sse, default http).
 */
export interface McpConfig extends K8sHints {
  kind: "mcp";
  binary: string;
  default_port?: number;
  transport?: "http" | "sse";
  args?: string[];
  /** Contract/ABI major this tool exposes. Defaults to 1 in EE. */
  interface_major?: number;
}

/**
 * `kind = atool` config — a binary daemon reached over the native gRPC
 * `ToolService`. Mirrors EE `AtoolConfig`; `transport` defaults to "grpc".
 */
export interface AtoolConfig extends K8sHints {
  kind: "atool";
  binary: string;
  default_port?: number;
  transport?: "grpc" | "http" | "sse";
  args?: string[];
  /** Contract/ABI major this tool exposes. Defaults to 1 in EE. */
  interface_major?: number;
}

/**
 * `kind = aagent` config. Mirrors EE `AagentConfig`. A skill collapses into
 * this shape with only `system_prompt` populated — there is no `tags` field and
 * no standalone skill kind.
 */
export interface AagentConfig {
  kind: "aagent";
  system_prompt: string;
  allowed_tools?: string[];
  /** Preferred model backend id (EE `model`). */
  model?: string;
  history_limit?: number;
  /** How this prompt composes with `extends` bases: "append" (default) | "replace". */
  prompt_mode?: "append" | "replace";
}

/** Either binary-tool config (mcp or atool). */
export type BinaryToolConfig = McpConfig | AtoolConfig;

/** Inline sub-agent embedded in a parent aagent's components[]. */
export interface InlineComponent {
  name: string;
  /** Canonical ns/name@version used for coalescing. */
  id: string;
  kind: "aagent";
  config: AagentConfig;
  components?: ComponentItem[];
  files?: FileEntry[];
  permissions?: Permissions;
  dependencies?: Dependency[];
}

/** External reference to an already-published package. */
export interface RefComponent {
  ref: string;
}

/** Discriminated union: inline sub-agent OR external ref. */
export type ComponentItem = InlineComponent | RefComponent;

/** Merge rules for flattening components into the root aagent at install time. */
export interface InstallFlatten {
  system_prompt?: "concat" | "root_wins" | "error_on_conflict";
  allowed_tools?: "union" | "root_wins";
  model?: "root_wins";
  history_limit?: "root_wins";
}

export interface InstallBlock {
  flatten?: InstallFlatten;
}

export interface SignatureBlock {
  alg: string;
  key_fingerprint: string;
  value: string;
  scope: "bundle" | "per_component";
}

export type PackageConfig = McpConfig | AtoolConfig | AagentConfig;

export interface Manifest {
  schema_version: "2";
  name: string;
  version: string;
  kind: Kind;
  description: string;
  author?: string;
  license?: string;
  requires_alexandria?: string;
  dependencies?: Dependency[];
  /** Base packages this aagent extends. aagent-only — rejected on mcp/atool. */
  extends?: PackageDep[];
  /** Resolved inheritance lockfile (aagent-only). */
  lockfile?: LockEntry[];
  config: PackageConfig;
  files?: FileEntry[];
  permissions?: Permissions;
  /** Inline sub-agent composition. Only valid on kind=aagent. */
  components?: ComponentItem[];
  /** Install-time merge rules. Only on aagents with non-empty components[]. */
  install?: InstallBlock;
  /** Signature block (EE-signed archives). */
  signature?: SignatureBlock;
}
