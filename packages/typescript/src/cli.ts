#!/usr/bin/env node
import { mkdirSync, writeFileSync, readdirSync, readFileSync, cpSync, copyFileSync, existsSync, statSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface, type Interface as ReadlineInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";
import { pack, verify, inspect } from "./pack.js";
import type { Manifest } from "./types.js";

const HERE = dirname(fileURLToPath(import.meta.url));
const TEMPLATES_CANDIDATES = [
  resolve(HERE, "templates"),
  resolve(HERE, "../../../templates"),
  resolve(HERE, "../../templates"),
];

function templatesRoot(): string {
  for (const p of TEMPLATES_CANDIDATES) if (existsSync(p)) return p;
  throw new Error("templates/ not found near " + HERE);
}

const HELP = `alex-sdk — author .atool / .aagent packages

USAGE
  alex-sdk new                       Interactive wizard: scaffold + (optionally) pack
  alex-sdk init <template> <dir>     Scaffold from a named template
  alex-sdk pack <src-dir> [-o out]   Pack into .atool or .aagent
  alex-sdk verify <pkg>              Re-hash files, validate manifest
  alex-sdk inspect <pkg>             Print manifest + file list
  alex-sdk migrate <src> [-o out]    Upgrade v1 atool.json to v2

TEMPLATES
  tool-node, tool-python, agent-basic, agent-collection

EXAMPLES
  alex-sdk new
  alex-sdk init agent-basic ./my-agent
  alex-sdk pack ./my-agent -o my-agent-0.1.0.aagent
  alex-sdk verify my-agent-0.1.0.aagent
  alex-sdk migrate old-atool.json -o atool.json
`;

function die(msg: string, code = 1): never { process.stderr.write(msg + "\n"); process.exit(code); }

function defaultOutPath(srcDir: string, manifestKind: string): string {
  const m = JSON.parse(readFileSync(join(srcDir, "atool.json"), "utf8"));
  const short = String(m.name).split("/").pop();
  const ext = manifestKind === "aagent" ? "aagent" : "atool";
  return `${short}-${m.version}.${ext}`;
}

/**
 * Migrate a v1 manifest object to the EE-canonical v2 taxonomy.
 * Returns { manifest, warnings, errors }.
 *
 * Kind remap:  tool -> mcp|atool (by transport: grpc=>atool, else mcp);
 *              skill -> aagent;  agent -> aagent;  bundle -> aagent.
 * Field remap: model stays model (EE uses `model`); model_hint/llm -> model;
 *              aagent tags dropped (no such field in EE AagentConfig).
 */
export function migrateManifest(v1: Record<string, unknown>): { manifest: Record<string, unknown>; warnings: string[]; errors: string[] } {
  const warnings: string[] = [];
  const errors: string[] = [];
  const m: Record<string, unknown> = { ...v1 };

  // Bump schema_version
  m.schema_version = "2";

  // Handle removed kinds
  const kind = m.kind as string;
  if (kind === "llm-runtime" || kind === "llm-backend") {
    errors.push(
      `kind '${kind}' has no v2 equivalent; register a model via \`alexandria install <name> --model\` (.amodel) instead`
    );
    return { manifest: m, warnings, errors };
  }

  if (kind === "bundle") {
    // A bundle collapses to an aagent orchestrator carrying components[] refs.
    m.kind = "aagent";
    warnings.push("bundle converted to aagent; add config.system_prompt before publishing");
    const bcfg = (m.config ?? {}) as Record<string, unknown>;
    const oldComponents = bcfg.components as string[] | undefined;
    if (Array.isArray(oldComponents)) {
      m.components = oldComponents.map((ref: string) => ({ ref }));
    }
    m.config = { kind: "aagent", system_prompt: "TODO: add system_prompt" };
  } else if (kind === "tool") {
    // A v1 tool becomes mcp (MCP JSON-RPC/SSE) or atool (native gRPC),
    // discriminated by transport. Default (no transport) is MCP over http.
    const cfg = (m.config ?? {}) as Record<string, unknown>;
    const newKind = cfg.transport === "grpc" ? "atool" : "mcp";
    m.kind = newKind;
    cfg.kind = newKind;
    m.config = cfg;
    if (newKind === "atool") {
      warnings.push("kind 'tool' with transport=grpc migrated to kind 'atool' (native ToolService)");
    } else {
      warnings.push("kind 'tool' migrated to kind 'mcp' (MCP JSON-RPC/SSE)");
    }
  } else if (kind === "skill") {
    // EE has no standalone skill kind — a skill is reusable prompt text that
    // ships as an aagent whose content is its system_prompt.
    m.kind = "aagent";
    const cfg = (m.config ?? {}) as Record<string, unknown>;
    cfg.kind = "aagent";
    m.config = cfg;
    warnings.push("kind 'skill' migrated to kind 'aagent' (skills live in aagent.system_prompt)");
  } else if (kind === "agent") {
    const cfg = (m.config ?? {}) as Record<string, unknown>;
    cfg.kind = "aagent";
    m.config = cfg;
    m.kind = "aagent";
  }

  // Migrate config fields to EE serde names.
  const cfg = (m.config ?? {}) as Record<string, unknown>;
  // EE uses `model`; the intermediate SDK-v2 field `llm` folds back to it.
  if ("llm" in cfg) {
    cfg.model = cfg.llm;
    delete cfg.llm;
    warnings.push("config.llm renamed to config.model");
  }
  if ("model_hint" in cfg) {
    cfg.model = cfg.model_hint;
    delete cfg.model_hint;
    warnings.push("config.model_hint renamed to config.model");
  }
  // Remove default_mode (no longer a field)
  if ("default_mode" in cfg) {
    delete cfg.default_mode;
    warnings.push("config.default_mode removed (swarm is always default)");
  }
  // EE AagentConfig has no `tags` field.
  if ("tags" in cfg) {
    delete cfg.tags;
    warnings.push("config.tags removed (EE aagent has no tags field)");
  }
  m.config = cfg;

  // Strip old signing fields at wrong locations
  const strippedSigning: string[] = [];
  for (const field of ["signed_at", "key_fingerprint", "signature"] as const) {
    if (field in m && field !== "signature") {
      delete (m as Record<string, unknown>)[field];
      strippedSigning.push(field);
    }
    // signature at wrong level (v1 extra property) gets removed too
  }
  // If 'signature' is present but not in the v2 shape, remove it
  if ("signature" in m) {
    const sig = m.signature as Record<string, unknown> | undefined;
    const hasV2Shape = sig && "alg" in sig && "key_fingerprint" in sig && "value" in sig && "scope" in sig;
    if (!hasV2Shape) {
      delete m.signature;
      strippedSigning.push("signature");
    }
  }
  if (strippedSigning.length > 0) {
    warnings.push(`signing fields removed (${strippedSigning.join(", ")}); re-sign after migration`);
  }

  // Warn about dependencies missing version
  const deps = m.dependencies as Array<Record<string, unknown>> | undefined;
  if (Array.isArray(deps)) {
    for (const dep of deps) {
      if (!dep.version) {
        warnings.push(`dependency '${dep.name ?? "?"}' missing version field; add before publishing`);
      }
    }
  }

  return { manifest: m, warnings, errors };
}

async function cmdMigrate(args: string[]) {
  const src = args[0];
  if (!src) die("usage: alex-sdk migrate <src> [-o <out>]");
  const oi = args.indexOf("-o");
  const outPath = oi >= 0 ? args[oi + 1] : undefined;

  // Read manifest — either from a directory (atool.json inside) or a JSON file
  let raw: string;
  let resolvedSrc: string;
  try {
    const st = statSync(src);
    if (st.isDirectory()) {
      resolvedSrc = join(src, "atool.json");
    } else {
      resolvedSrc = src;
    }
    raw = readFileSync(resolvedSrc, "utf8");
  } catch (e: unknown) {
    die(`cannot read ${src}: ${(e as Error).message}`);
  }

  let v1: Record<string, unknown>;
  try {
    v1 = JSON.parse(raw!);
  } catch (e: unknown) {
    die(`invalid JSON in ${src}: ${(e as Error).message}`);
  }

  const { manifest, warnings, errors } = migrateManifest(v1!);

  if (errors.length > 0) {
    process.stderr.write("Migration errors:\n");
    for (const e of errors) process.stderr.write(`  ERROR: ${e}\n`);
    process.exit(1);
  }

  const json = JSON.stringify(manifest, null, 2) + "\n";
  const dest = outPath ?? resolvedSrc!;
  writeFileSync(dest, json, "utf8");

  if (warnings.length > 0) {
    process.stderr.write("Migration warnings:\n");
    for (const w of warnings) process.stderr.write(`  WARN: ${w}\n`);
  }
  process.stdout.write(`Migrated to v2 -> ${dest}\n`);
}

// Build a question-asker over a readline interface. We can't use rl.question()
// because readline/promises only resolves it once when stdin is piped (non-TTY),
// and we can't use rl.once("line") because lines from a piped stdin can buffer
// and fire before the handler attaches. Instead, queue every line as it comes
// in and let ask() pull from the queue. Works identically in TTY and piped.
function makeAsker(rl: ReadlineInterface) {
  const queue: string[] = [];
  const waiters: Array<(line: string) => void> = [];
  let closed = false;
  rl.on("line", (line) => {
    if (waiters.length) waiters.shift()!(line);
    else queue.push(line);
  });
  rl.on("close", () => {
    closed = true;
    while (waiters.length) waiters.shift()!("");
  });
  const readLine = async (): Promise<string> => {
    if (queue.length) return queue.shift()!;
    if (closed) return "";
    return new Promise<string>((r) => waiters.push(r));
  };
  return {
    ask: async (q: string, def?: string): Promise<string> => {
      const suffix = def !== undefined && def !== "" ? ` [${def}]` : "";
      stdout.write(`${q}${suffix}: `);
      const ans = (await readLine()).trim();
      return ans === "" ? (def ?? "") : ans;
    },
    askYes: async (q: string, defYes = true): Promise<boolean> => {
      stdout.write(`${q} [${defYes ? "Y/n" : "y/N"}]: `);
      const ans = (await readLine()).trim().toLowerCase();
      if (ans === "") return defYes;
      return ans === "y" || ans === "yes";
    },
  };
}

/**
 * Interactive scaffold. For tools, wraps an existing binary on disk into a
 * minimal .atool. For agents/skills, scaffolds atool.json with the given
 * system_prompt (inline or from file). Optionally packs at the end.
 */
async function cmdNew(args: string[]) {
  // -y / --yes: future non-interactive flag hook; for now we still prompt but
  // skip the final pack confirmation. Reserved for scripting.
  const autoPack = args.includes("-y") || args.includes("--yes");

  const rl = createInterface({ input: stdin, output: stdout });
  const { ask, askYes } = makeAsker(rl);
  try {
    stdout.write("alex-sdk new — minimal interactive scaffold\n\n");

    // Friendly authoring kinds map to EE kinds: tool -> mcp|atool (by transport),
    // agent -> aagent, skill -> aagent (prompt-only).
    const authorKind = await ask("Kind (tool/agent/skill)", "tool");
    if (!["tool", "agent", "skill"].includes(authorKind)) die(`invalid kind '${authorKind}'`);

    const name = await ask("Name (ns/name)");
    if (!name) die("name is required");
    if (!name.includes("/")) {
      stdout.write(`  (warning: name '${name}' is not in ns/name form; EE coalescing keys off the full id)\n`);
    }
    const short = name.split("/").pop()!;

    const version = await ask("Version", "0.1.0");
    const description = await ask("Description", `${authorKind} ${name}`);
    const author = await ask("Author (optional)", "");
    const license = await ask("License (optional)", "");

    const outDirDefault = `./${short}`;
    const outDir = resolve(await ask("Source directory to create", outDirDefault));
    if (existsSync(outDir) && readdirSync(outDir).length > 0) {
      const ok = await askYes(`  '${outDir}' is non-empty. Continue and overwrite atool.json?`, false);
      if (!ok) die("aborted");
    }
    mkdirSync(outDir, { recursive: true });

    const manifest: Record<string, unknown> = {
      schema_version: "2",
      name,
      version,
      // EE kind — filled in below once transport (for tools) is known.
      kind: "",
      description,
    };
    if (author) manifest.author = author;
    if (license) manifest.license = license;

    const files: Array<Record<string, unknown>> = [];

    if (authorKind === "tool") {
      const binSrc = await ask("Binary path on disk (will be staged)");
      if (!binSrc) die("binary path is required for kind=tool");
      const binAbs = resolve(binSrc);
      if (!existsSync(binAbs) || !statSync(binAbs).isFile()) die(`binary not found: ${binAbs}`);

      const binName = basename(binAbs);
      const archivePath = `bin/${binName}`;
      const installPath = await ask("Install path inside the runtime sandbox", archivePath);
      mkdirSync(join(outDir, "bin"), { recursive: true });
      copyFileSync(binAbs, join(outDir, archivePath));
      files.push({ archive_path: archivePath, install_path: installPath, executable: true });

      const portStr = await ask("Default port (0-65535, optional)", "");
      // Transport picks the EE kind: grpc => atool (native ToolService),
      // http/sse => mcp (MCP JSON-RPC/SSE).
      const transport = await ask("Transport (grpc/http/sse)", "grpc");
      if (!["grpc", "http", "sse"].includes(transport)) die(`invalid transport '${transport}'`);
      const emitKind = transport === "grpc" ? "atool" : "mcp";
      manifest.kind = emitKind;

      const config: Record<string, unknown> = {
        kind: emitKind,
        binary: archivePath,
        transport,
      };
      if (portStr) {
        const p = Number(portStr);
        if (!Number.isFinite(p) || p < 0 || p > 65535) die(`invalid port '${portStr}'`);
        config.default_port = p;
      }
      manifest.config = config;
    } else {
      // agent or skill — both emit kind=aagent. A skill is prompt-only.
      manifest.kind = "aagent";
      let systemPrompt = "";
      const promptSrc = await ask("System prompt — path to a .md/.txt file, or empty to type inline", "");
      if (promptSrc) {
        const abs = resolve(promptSrc);
        if (!existsSync(abs)) die(`system prompt file not found: ${abs}`);
        systemPrompt = readFileSync(abs, "utf8");
      } else {
        systemPrompt = await ask("Inline system_prompt", `You are ${name}.`);
      }
      const allowedRaw = await ask("Allowed tools (comma-separated, optional)", "");
      const model = await ask("Preferred model id (optional)", "");

      const config: Record<string, unknown> = {
        kind: "aagent",
        system_prompt: systemPrompt,
      };
      if (allowedRaw) config.allowed_tools = allowedRaw.split(",").map(s => s.trim()).filter(Boolean);
      if (model) config.model = model;

      // History limit applies to both agents and prompt-only skills (aagent).
      const histStr = await ask("History limit (optional)", "");
      if (histStr) {
        const n = Number(histStr);
        if (!Number.isFinite(n) || n < 1) die(`invalid history_limit '${histStr}'`);
        config.history_limit = n;
      }
      manifest.config = config;
    }

    if (files.length > 0) manifest.files = files;

    const manifestPath = join(outDir, "atool.json");
    writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n", "utf8");
    stdout.write(`\nWrote ${manifestPath}\n`);

    const doPack = autoPack ? true : await askYes("Pack now?", true);
    if (doPack) {
      const ext = manifest.kind === "aagent" ? "aagent" : "atool";
      const out = await ask("Output archive", `${short}-${version}.${ext}`);
      const packed = await pack(outDir, out);
      stdout.write(`Packed ${packed.name}@${packed.version} -> ${out}\n`);
    } else {
      stdout.write(`Next: alex-sdk pack ${outDir}\n`);
    }
  } finally {
    rl.close();
  }
}

async function main() {
  const [cmd, ...rest] = process.argv.slice(2);
  if (!cmd || cmd === "-h" || cmd === "--help") { process.stdout.write(HELP); return; }

  switch (cmd) {
    case "init": {
      const [tpl, dir] = rest;
      if (!tpl || !dir) die("usage: alex-sdk init <template> <dir>");
      const src = join(templatesRoot(), tpl);
      if (!existsSync(src)) die(`unknown template '${tpl}'. Available: ${readdirSync(templatesRoot()).join(", ")}`);
      mkdirSync(dir, { recursive: true });
      cpSync(src, dir, { recursive: true });
      process.stdout.write(`Scaffolded ${tpl} into ${dir}\nEdit atool.json, then: alex-sdk pack ${dir}\n`);
      return;
    }
    case "pack": {
      const srcDir = rest[0];
      if (!srcDir) die("usage: alex-sdk pack <src-dir> [-o out]");
      const oi = rest.indexOf("-o");
      const m = JSON.parse(readFileSync(join(srcDir, "atool.json"), "utf8"));
      const out = oi >= 0 ? rest[oi + 1] : defaultOutPath(srcDir, m.kind);
      const manifest = await pack(srcDir, out);
      process.stdout.write(`Packed ${manifest.name}@${manifest.version} -> ${out}\n`);
      return;
    }
    case "verify": {
      const pkg = rest[0];
      if (!pkg) die("usage: alex-sdk verify <pkg>");
      const m = await verify(pkg);
      process.stdout.write(`OK ${m.name}@${m.version} (kind=${m.kind})\n`);
      return;
    }
    case "inspect": {
      const pkg = rest[0];
      if (!pkg) die("usage: alex-sdk inspect <pkg>");
      const r = await inspect(pkg);
      process.stdout.write(JSON.stringify(r, null, 2) + "\n");
      return;
    }
    case "migrate": {
      await cmdMigrate(rest);
      return;
    }
    case "new": {
      await cmdNew(rest);
      return;
    }
    default:
      die(`unknown command '${cmd}'\n\n${HELP}`);
  }
}

main().catch(e => die(String(e?.stack ?? e)));
