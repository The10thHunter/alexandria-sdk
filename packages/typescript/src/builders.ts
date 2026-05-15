import { writeFileSync, mkdtempSync, mkdirSync, copyFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { pack } from "./pack.js";
import { assertValid } from "./schema.js";
import type {
  AgentConfig, ComponentItem, Dependency, FileEntry, InlineComponent, InstallBlock, InstallFlatten,
  K8sResources, Manifest, Permissions, SkillConfig, ToolConfig,
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

export class Tool extends Base<ToolConfig> {
  declare protected manifest: Manifest & { config: ToolConfig };
  constructor(name: string, version: string) {
    super(name, version, "tool", { kind: "tool", binary: "" });
  }
  binary(p: string): this { this.manifest.config.binary = p; return this; }
  port(p: number): this { this.manifest.config.default_port = p; return this; }
  transport(t: "http" | "sse"): this { this.manifest.config.transport = t; return this; }
  args(a: string[]): this { this.manifest.config.args = a; return this; }
  k8sImage(img: string): this { this.manifest.config.k8s_image = img; return this; }
  k8sCapabilities(c: string[]): this { this.manifest.config.k8s_capabilities = c; return this; }
  k8sPort(p: number): this { this.manifest.config.k8s_port = p; return this; }
  k8sTransport(t: "grpc" | "http" | "sse"): this { this.manifest.config.k8s_transport = t; return this; }
  k8sResources(r: K8sResources): this { this.manifest.config.k8s_resources = r; return this; }
  k8sMinWarm(n: number): this { this.manifest.config.k8s_min_warm = n; return this; }
  k8sIdleTimeout(seconds: number): this { this.manifest.config.k8s_idle_timeout_seconds = seconds; return this; }
}

export class Agent extends Base<AgentConfig> {
  declare protected manifest: Manifest & { config: AgentConfig };
  constructor(name: string, version: string) {
    super(name, version, "agent", { kind: "agent", system_prompt: "" });
  }
  systemPrompt(s: string): this { this.manifest.config.system_prompt = s; return this; }
  systemPromptFromFile(p: string): this {
    this.manifest.config.system_prompt = readFileSync(p, "utf8");
    return this;
  }
  allowedTools(t: string[]): this { this.manifest.config.allowed_tools = t; return this; }
  /** Replaces v1 `.model()`. Sets config.llm (freeform preference). */
  llm(id: string): this { this.manifest.config.llm = id; return this; }
  historyLimit(n: number): this { this.manifest.config.history_limit = n; return this; }

  /**
   * Append an inline sub-agent or sub-skill component.
   * `name` is the local label; `id` is the canonical ns/name@version.
   */
  component(name: string, id: string, child: Agent | Skill): this {
    const childManifest = child.build();
    const item: InlineComponent = {
      name,
      id,
      kind: childManifest.kind as "agent" | "skill",
      config: childManifest.config as AgentConfig | SkillConfig,
    };
    if (childManifest.files) item.files = childManifest.files;
    if (childManifest.permissions) item.permissions = childManifest.permissions;
    if (childManifest.dependencies) item.dependencies = childManifest.dependencies;
    if (childManifest.kind === "agent" && childManifest.components) {
      item.components = childManifest.components;
    }
    (this.manifest.components ??= []).push(item);
    return this;
  }

  /**
   * Append an external ref component (any kind: tool, skill, or agent).
   * Tools may ONLY appear as refs (never inline) per the schema.
   */
  ref(nsNameAtVersion: string): this {
    (this.manifest.components ??= []).push({ ref: nsNameAtVersion });
    return this;
  }

  /**
   * Set install.flatten merge rules (only meaningful on agents with components[]).
   */
  flatten(rules: InstallFlatten): this {
    (this.manifest.install ??= {}).flatten = rules;
    return this;
  }
}

export class Skill extends Base<SkillConfig> {
  declare protected manifest: Manifest & { config: SkillConfig };
  constructor(name: string, version: string) {
    super(name, version, "skill", { kind: "skill", system_prompt: "" });
  }
  systemPrompt(s: string): this { this.manifest.config.system_prompt = s; return this; }
  allowedTools(t: string[]): this { this.manifest.config.allowed_tools = t; return this; }
  /** Replaces v1 `.modelHint()`. Sets config.llm (freeform preference). */
  llm(id: string): this { this.manifest.config.llm = id; return this; }
  tags(t: string[]): this { this.manifest.config.tags = t; return this; }
}
