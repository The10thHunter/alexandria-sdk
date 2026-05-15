export type Kind = "tool" | "agent" | "skill";

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

export interface Dependency {
  name: string;
  version: string;
}

export interface K8sResources {
  requests?: { cpu?: string; memory?: string };
  limits?: { cpu?: string; memory?: string };
}

export interface ToolConfig {
  kind: "tool";
  binary: string;
  default_port?: number;
  transport?: "http" | "sse";
  args?: string[];
  k8s_image?: string;
  k8s_capabilities?: string[];
  k8s_port?: number;
  k8s_transport?: "grpc" | "http" | "sse";
  k8s_resources?: K8sResources;
  k8s_min_warm?: number;
  k8s_idle_timeout_seconds?: number;
}

export interface AgentConfig {
  kind: "agent";
  system_prompt: string;
  allowed_tools?: string[];
  /** Preferred LLM id (freeform, preference only). Replaces v1 `model`. */
  llm?: string;
  history_limit?: number;
}

export interface SkillConfig {
  kind: "skill";
  system_prompt: string;
  allowed_tools?: string[];
  /** Preferred LLM id (freeform, preference only). Replaces v1 `model_hint`. */
  llm?: string;
  tags?: string[];
}

/** Inline sub-agent or sub-skill embedded in a parent agent's components[]. */
export interface InlineComponent {
  name: string;
  /** Canonical ns/name@version used for coalescing. */
  id: string;
  kind: "agent" | "skill";
  config: AgentConfig | SkillConfig;
  components?: ComponentItem[];
  files?: FileEntry[];
  permissions?: Permissions;
  dependencies?: Dependency[];
}

/** External reference to an already-published package. */
export interface RefComponent {
  ref: string;
}

/** Discriminated union: inline sub-agent/skill OR external ref. */
export type ComponentItem = InlineComponent | RefComponent;

/** Merge rules for flattening components into the root agent at install time. */
export interface InstallFlatten {
  system_prompt?: "concat" | "root_wins" | "error_on_conflict";
  allowed_tools?: "union" | "root_wins";
  llm?: "root_wins";
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

export type PackageConfig = ToolConfig | AgentConfig | SkillConfig;

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
  files?: FileEntry[];
  permissions?: Permissions;
  config: PackageConfig;
  /** Only valid on kind=agent with non-empty components[]. */
  components?: ComponentItem[];
  /** Install-time merge rules. Only on agents with non-empty components[]. */
  install?: InstallBlock;
  /** Signature block (EE-signed archives). */
  signature?: SignatureBlock;
}
