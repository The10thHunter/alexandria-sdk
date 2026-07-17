import { writeFileSync, mkdtempSync, mkdirSync, copyFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { pack } from "./pack.js";
import { assertValid } from "./schema.js";
import type {
  AagentConfig, AtoolConfig, BundleConfig, ComponentItem, CredentialDecl, Dependency, EnvDecl, FileEntry,
  InlineComponent, InstallFlatten, K8sResources, LockEntry, Manifest, McpConfig, PackageDep,
  Permissions,
} from "./types.js";

abstract class Base<TConfig extends Manifest["config"]> {
  protected manifest: Manifest;

  constructor(name: string, version: string, kind: Manifest["kind"], config: TConfig) {
    this.manifest = {
      schema_version: "2",
      name,
      version,
      kind,
      description: "",
      config: config as Manifest["config"],
    };
  }

  description(d: string): this { this.manifest.description = d; return this; }
  author(a: string): this { this.manifest.author = a; return this; }
  license(l: string): this { this.manifest.license = l; return this; }
  requiresAlexandria(v: string): this { this.manifest.requires_alexandria = v; return this; }

  dependency(d: Dependency): this {
    (this.manifest.dependencies ??= []).push(d);
    return this;
  }
  dependencies(ds: Dependency[]): this { this.manifest.dependencies = ds; return this; }

  file(f: FileEntry): this { (this.manifest.files ??= []).push(f); return this; }
  files(fs: FileEntry[]): this { this.manifest.files = fs; return this; }

  protected ensurePerms(): Permissions {
    return (this.manifest.permissions ??= {});
  }
  providesTools(t: string[]): this { this.ensurePerms().provides_tools = t; return this; }
  needsTools(t: string[]): this { this.ensurePerms().needs_tools = t; return this; }
  suggestedRole(r: string): this { this.ensurePerms().suggested_role = r; return this; }

  build(): Manifest {
    return assertValid(JSON.parse(JSON.stringify(this.manifest)));
  }

  /**
   * Pack to a `.atool` / `.aagent`. By default, materialises a temp source dir
   * with `atool.json` and (for builders that staged file content) any staged
   * files; advanced callers can pass an existing source dir via `opts.srcDir`.
   */
  async pack(outPath: string, opts: { srcDir?: string } = {}): Promise<Manifest> {
    if (opts.srcDir) {
      const m = this.build();
      writeFileSync(join(opts.srcDir, "atool.json"), JSON.stringify(m, null, 2) + "\n");
      return pack(opts.srcDir, outPath);
    }
    const dir = mkdtempSync(join(tmpdir(), "alex-sdk-"));
    const m = this.build();
    writeFileSync(join(dir, "atool.json"), JSON.stringify(m, null, 2) + "\n");
    for (const [archivePath, srcAbs] of this.staged) {
      const dest = join(dir, archivePath);
      mkdirSync(dirname(dest), { recursive: true });
      copyFileSync(srcAbs, dest);
    }
    return pack(dir, outPath);
  }

  protected staged: Array<[string, string]> = [];
  /**
   * Stage a file from disk so `.pack()` can include it without a pre-laid-out
   * source dir. Automatically appends a matching `files[]` entry.
   */
  stageFile(srcPath: string, entry: Omit<FileEntry, "sha256">): this {
    this.staged.push([entry.archive_path, resolve(srcPath)]);
    this.file(entry);
    return this;
  }
}

/**
 * Binary tool builder. Emits `kind = atool` (native gRPC `ToolService`) by
 * default; calling `.transport("http" | "sse")` re-taxes the package to
 * `kind = mcp` (MCP JSON-RPC/SSE). `.transport("grpc")` keeps it an atool.
 * This mirrors EE: a "tool" is either an mcp daemon or an atool daemon,
 * discriminated by the wire protocol the orchestrator speaks.
 */
export class Tool extends Base<McpConfig | AtoolConfig> {
  declare protected manifest: Manifest & { config: McpConfig | AtoolConfig };
  constructor(name: string, version: string) {
    // Default transport is gRPC => atool, matching EE's AtoolConfig default.
    super(name, version, "atool", { kind: "atool", binary: "" });
  }
  binary(p: string): this { this.manifest.config.binary = p; return this; }
  port(p: number): this { this.manifest.config.default_port = p; return this; }
  /**
   * Pick the wire protocol — and thereby the package kind:
   *   "grpc"       => kind atool (native ToolService)
   *   "http"|"sse" => kind mcp   (MCP JSON-RPC/SSE)
   */
  transport(t: "grpc" | "http" | "sse"): this {
    const kind = t === "grpc" ? "atool" : "mcp";
    this.manifest.kind = kind;
    this.manifest.config.kind = kind;
    this.manifest.config.transport = t;
    return this;
  }
  args(a: string[]): this { this.manifest.config.args = a; return this; }
  /** Contract/ABI major this tool exposes over its wire protocol (EE default 1). */
  interfaceMajor(n: number): this { this.manifest.config.interface_major = n; return this; }

  /**
   * Declare this as a **code-less** tool that binds to a native orchestrator
   * handler INSTEAD of shipping a binary (closed set, currently
   * `"emit_trigger"`). Drops the default-seeded empty `binary`. A code-less tool
   * must also declare its {@link Tool.inputSchema} — there is no daemon to
   * advertise it.
   */
  nativeHandler(name: string): this {
    const cfg = this.manifest.config as AtoolConfig;
    cfg.native_handler = name;
    delete cfg.binary;
    return this;
  }

  /**
   * Declare the tool's full input contract as an embedded JSON Schema object.
   * Required for a code-less tool ({@link Tool.nativeHandler}); optional static
   * fallback for a coded tool.
   */
  inputSchema(schema: Record<string, unknown>): this {
    (this.manifest.config as AtoolConfig).input_schema = schema;
    return this;
  }

  /**
   * Declare a secret credential this tool reads from an environment variable.
   * No secret *value* is ever placed in the package — the operator binds the
   * value into the deployment-shape secret backend at install time. `secret`
   * defaults `true`, `rotation` defaults `"respawn"`.
   */
  credential(
    env: string,
    opts: { required?: boolean; secret?: boolean; description?: string; rotation?: "respawn" | "oauth-refresh" } = {},
  ): this {
    const c: CredentialDecl = {
      env,
      secret: opts.secret ?? true,
      required: opts.required ?? false,
      rotation: opts.rotation ?? "respawn",
    };
    if (opts.description) c.description = opts.description;
    (this.manifest.config.credentials ??= []).push(c);
    return this;
  }

  /**
   * Declare a non-secret config environment variable this tool reads, with an
   * optional literal `default`. Values are stored inline by the operator, not as
   * a secret ref.
   */
  env(name: string, opts: { default?: string; required?: boolean } = {}): this {
    const e: EnvDecl = { name };
    if (opts.default !== undefined) e.default = opts.default;
    if (opts.required) e.required = true;
    (this.manifest.config.env ??= []).push(e);
    return this;
  }
  k8sImage(img: string): this { this.manifest.config.k8s_image = img; return this; }
  k8sCapabilities(c: string[]): this { this.manifest.config.k8s_capabilities = c; return this; }
  k8sPort(p: number): this { this.manifest.config.k8s_port = p; return this; }
  k8sTransport(t: "grpc" | "http" | "sse"): this { this.manifest.config.k8s_transport = t; return this; }
  k8sResources(r: K8sResources): this { this.manifest.config.k8s_resources = r; return this; }
  k8sMinWarm(n: number): this { this.manifest.config.k8s_min_warm = n; return this; }
  k8sIdleTimeout(seconds: number): this { this.manifest.config.k8s_idle_timeout_seconds = seconds; return this; }
}

export class Agent extends Base<AagentConfig> {
  declare protected manifest: Manifest & { config: AagentConfig };
  constructor(name: string, version: string) {
    super(name, version, "aagent", { kind: "aagent", system_prompt: "" });
  }
  systemPrompt(s: string): this { this.manifest.config.system_prompt = s; return this; }
  systemPromptFromFile(p: string): this {
    this.manifest.config.system_prompt = readFileSync(p, "utf8");
    return this;
  }
  allowedTools(t: string[]): this { this.manifest.config.allowed_tools = t; return this; }
  /** Preferred model backend id (EE `config.model`). Replaces v1 `.llm()`. */
  model(id: string): this { this.manifest.config.model = id; return this; }
  historyLimit(n: number): this { this.manifest.config.history_limit = n; return this; }
  /** Prompt composition mode against `extends` bases: "append" (default) | "replace". */
  promptMode(m: "append" | "replace"): this { this.manifest.config.prompt_mode = m; return this; }

  /**
   * Append a base package this aagent extends. aagent-only in EE — the flatten
   * resolver composes base→child. Rejected by validation on mcp/atool kinds.
   */
  extend(base: PackageDep): this {
    (this.manifest.extends ??= []).push(base);
    return this;
  }
  extendsPackages(bases: PackageDep[]): this { this.manifest.extends = bases; return this; }

  /** Append a resolved inheritance lockfile entry (aagent-only). */
  lock(entry: LockEntry): this {
    (this.manifest.lockfile ??= []).push(entry);
    return this;
  }
  lockfile(entries: LockEntry[]): this { this.manifest.lockfile = entries; return this; }

  /**
   * Append an inline sub-agent component.
   * `name` is the local label; `id` is the canonical ns/name@version.
   * Both Agent and Skill children emit kind=aagent.
   */
  component(name: string, id: string, child: Agent | Skill): this {
    const childManifest = child.build();
    const item: InlineComponent = {
      name,
      id,
      kind: "aagent",
      config: childManifest.config as AagentConfig,
    };
    if (childManifest.files) item.files = childManifest.files;
    if (childManifest.permissions) item.permissions = childManifest.permissions;
    if (childManifest.dependencies) item.dependencies = childManifest.dependencies;
    if (childManifest.components) item.components = childManifest.components;
    (this.manifest.components ??= []).push(item);
    return this;
  }

  /**
   * Append an external ref component (any kind: mcp/atool tool, or aagent).
   * Binary tools may ONLY appear as refs (never inline) per the schema.
   */
  ref(nsNameAtVersion: string): this {
    (this.manifest.components ??= []).push({ ref: nsNameAtVersion });
    return this;
  }

  /**
   * Set install.flatten merge rules (only meaningful on aagents with components[]).
   */
  flatten(rules: InstallFlatten): this {
    (this.manifest.install ??= {}).flatten = rules;
    return this;
  }
}

/**
 * Skill builder. A "skill" is reusable prompt text — EE has no standalone skill
 * kind, so this emits `kind = aagent` whose only content is `system_prompt`.
 */
export class Skill extends Base<AagentConfig> {
  declare protected manifest: Manifest & { config: AagentConfig };
  constructor(name: string, version: string) {
    super(name, version, "aagent", { kind: "aagent", system_prompt: "" });
  }
  systemPrompt(s: string): this { this.manifest.config.system_prompt = s; return this; }
  systemPromptFromFile(p: string): this {
    this.manifest.config.system_prompt = readFileSync(p, "utf8");
    return this;
  }
  allowedTools(t: string[]): this { this.manifest.config.allowed_tools = t; return this; }
  /** Preferred model backend id (EE `config.model`). Replaces v1 `.modelHint()`. */
  model(id: string): this { this.manifest.config.model = id; return this; }
}

/**
 * Bundle builder. A bundle is a NON-callable named set of member tools — the
 * unit a "role" (doer/delegator/file-handler) is made of. It ships no binary,
 * no native_handler, no input_schema, no model, no system_prompt; it is pure
 * composition. Its doctrine/"skill" (the stance) lives in the top-level
 * `.description(...)`. Emits `kind = "bundle"` with `config.tools`.
 */
export class Bundle extends Base<BundleConfig> {
  declare protected manifest: Manifest & { config: BundleConfig };
  constructor(name: string, version: string) {
    super(name, version, "bundle", { kind: "bundle", tools: [] });
  }
  /** Add one member tool reference (optionally `name@major`). */
  tool(ref: string): this { this.manifest.config.tools.push(ref); return this; }
  /** Replace the member tool list. At least one is required by the schema. */
  tools(refs: string[]): this { this.manifest.config.tools = [...refs]; return this; }
}
