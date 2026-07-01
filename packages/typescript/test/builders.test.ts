import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agent, Skill, Tool, verify, inspect, validate } from "../src/index.js";
import { migrateManifest } from "../src/cli.js";

test("Agent builder packs a valid .aagent and round-trips through verify", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const out = join(dir, "ra-0.1.0.aagent");
  await new Agent("acme/research", "0.1.0")
    .description("Research assistant")
    .systemPrompt("You are a senior research assistant.")
    .allowedTools(["web-search"])
    .model("claude-opus-4-7")
    .historyLimit(50)
    .pack(out);

  const m = await verify(out);
  assert.equal(m.kind, "aagent");
  assert.equal(m.name, "acme/research");
  assert.equal(m.schema_version, "2");
  if (m.config.kind === "aagent") assert.equal(m.config.model, "claude-opus-4-7");
});

test("Tool builder defaults to kind=atool (native gRPC ToolService)", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const src = join(dir, "src");
  mkdirSync(join(src, "bin"), { recursive: true });
  writeFileSync(join(src, "bin", "mytool"), "#!/bin/sh\nexit 0\n");

  const out = join(dir, "mytool-0.1.0.atool");
  await new Tool("acme/mytool", "0.1.0")
    .description("native gRPC daemon")
    .binary("bin/mytool")
    .interfaceMajor(2)
    .file({ archive_path: "bin/mytool", install_path: "tools/mytool/bin/mytool", executable: true })
    .pack(out, { srcDir: src });

  const r = await inspect(out);
  assert.equal(r.manifest.kind, "atool");
  assert.equal(r.manifest.config.kind, "atool");
  if (r.manifest.config.kind === "atool") assert.equal(r.manifest.config.interface_major, 2);
});

test("Tool builder emits kind=mcp when transport is http/sse", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const src = join(dir, "src");
  mkdirSync(join(src, "bin"), { recursive: true });
  writeFileSync(join(src, "bin", "mytool"), "#!/bin/sh\nexit 0\n");

  const out = join(dir, "mytool-0.1.0.atool");
  await new Tool("acme/mytool", "0.1.0")
    .description("mcp daemon")
    .binary("bin/mytool").port(7800).transport("http")
    .file({ archive_path: "bin/mytool", install_path: "tools/mytool/bin/mytool", executable: true })
    .pack(out, { srcDir: src });

  const r = await inspect(out);
  assert.equal(r.manifest.kind, "mcp");
  assert.equal(r.manifest.config.kind, "mcp");
  assert.equal(r.manifest.schema_version, "2");
  const entry = r.manifest.files!.find(f => f.archive_path === "bin/mytool")!;
  assert.match(entry.sha256!, /^[a-f0-9]{64}$/);
});

test("Tool builder stays atool when transport is grpc", () => {
  const m = new Tool("acme/g", "0.1.0")
    .description("grpc tool")
    .binary("bin/g").transport("grpc")
    .build();
  assert.equal(m.kind, "atool");
  assert.equal(m.config.kind, "atool");
});

test("Skill emits kind=aagent (prompt-only, no tags)", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const out = join(dir, "skill-0.1.0.aagent");

  await new Skill("acme/my-skill", "0.1.0")
    .description("a reusable prompt skill")
    .systemPrompt("You are a specialized skill.")
    .model("claude-haiku")
    .pack(out);

  const m = await verify(out);
  assert.equal(m.kind, "aagent");
  assert.equal(m.schema_version, "2");
  if (m.config.kind === "aagent") {
    assert.equal(m.config.model, "claude-haiku");
    assert.equal(m.config.system_prompt, "You are a specialized skill.");
    assert.ok(!("tags" in m.config), "aagent config must not carry tags");
  }
});

test("Agent with extends + lockfile round-trips", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const out = join(dir, "child-0.1.0.aagent");

  await new Agent("acme/child", "0.1.0")
    .description("child agent extending a base")
    .systemPrompt("You extend a base agent.")
    .promptMode("append")
    .extend({ name: "acme/base-agent", version: "1.0.0" })
    .lock({ name: "web-search", interface_major: 2 })
    .pack(out);

  const m = await verify(out);
  assert.equal(m.kind, "aagent");
  assert.deepEqual(m.extends, [{ name: "acme/base-agent", version: "1.0.0" }]);
  assert.equal(m.lockfile!.length, 1);
  assert.equal(m.lockfile![0].name, "web-search");
  assert.equal(m.lockfile![0].interface_major, 2);
  if (m.config.kind === "aagent") assert.equal(m.config.prompt_mode, "append");
});

test("validation rejects extends on a binary tool (atool)", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/bad-tool",
    version: "0.1.0",
    kind: "atool",
    description: "atool that wrongly carries extends",
    config: { kind: "atool", binary: "bin/x" },
    extends: [{ name: "acme/base", version: "1.0.0" }],
  };
  const result = validate(manifest);
  assert.equal(result.ok, false, "extends must be rejected on atool");
});

test("validation rejects extends on a binary tool (mcp)", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/bad-mcp",
    version: "0.1.0",
    kind: "mcp",
    description: "mcp that wrongly carries extends",
    config: { kind: "mcp", binary: "bin/x" },
    extends: [{ name: "acme/base", version: "1.0.0" }],
  };
  const result = validate(manifest);
  assert.equal(result.ok, false, "extends must be rejected on mcp");
});

test("Agent with inline sub-agent component round-trips", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const out = join(dir, "parent-0.1.0.aagent");

  const child = new Agent("acme/child", "0.1.0")
    .description("child agent")
    .systemPrompt("You are a child agent.");

  await new Agent("acme/parent", "0.1.0")
    .description("parent agent with component")
    .systemPrompt("You orchestrate sub-agents.")
    .component("child-agent", "acme/child@0.1.0", child)
    .pack(out);

  const m = await verify(out);
  assert.equal(m.kind, "aagent");
  assert.ok(m.components, "components should be present");
  assert.equal(m.components!.length, 1);
  const comp = m.components![0];
  assert.ok("name" in comp && comp.name === "child-agent", "inline component has name");
  assert.ok("id" in comp && comp.id === "acme/child@0.1.0", "inline component has id");
  assert.ok("kind" in comp && comp.kind === "aagent", "inline component is aagent");
});

test("Skill can be embedded inline as an aagent component", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const out = join(dir, "parent-skill-0.1.0.aagent");

  const skill = new Skill("acme/skill", "0.1.0")
    .description("prompt skill")
    .systemPrompt("Reusable prompt text.");

  await new Agent("acme/parent-skill", "0.1.0")
    .description("parent embedding a skill")
    .systemPrompt("You compose a skill.")
    .component("my-skill", "acme/skill@0.1.0", skill)
    .pack(out);

  const m = await verify(out);
  const comp = m.components![0];
  assert.ok("kind" in comp && comp.kind === "aagent", "skill embeds as aagent");
});

test("Agent with ref component round-trips", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const out = join(dir, "parent-ref-0.1.0.aagent");

  await new Agent("acme/parent-ref", "0.1.0")
    .description("parent with ref")
    .systemPrompt("You orchestrate via refs.")
    .ref("acme/some-tool@1.0.0")
    .ref("acme/some-agent@2.0.0")
    .pack(out);

  const m = await verify(out);
  assert.equal(m.components!.length, 2);
  assert.ok("ref" in m.components![0] && m.components![0].ref === "acme/some-tool@1.0.0");
});

test("Agent with flatten rules round-trips", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const out = join(dir, "flat-0.1.0.aagent");

  await new Agent("acme/flat", "0.1.0")
    .description("agent with flatten")
    .systemPrompt("You merge sub-agents.")
    .ref("acme/sub@1.0.0")
    .flatten({ system_prompt: "concat", allowed_tools: "union" })
    .pack(out);

  const m = await verify(out);
  assert.equal(m.install?.flatten?.system_prompt, "concat");
  assert.equal(m.install?.flatten?.allowed_tools, "union");
});

test("validation rejects components on a binary tool kind", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/bad-tool",
    version: "0.1.0",
    kind: "atool",
    description: "a tool that wrongly has components",
    config: { kind: "atool", binary: "bin/x" },
    components: [{ ref: "acme/foo@1.0" }],
  };
  const result = validate(manifest);
  assert.equal(result.ok, false, "should reject components on atool");
});

test("validation rejects inline binary tool in components", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/bad-agent",
    version: "0.1.0",
    kind: "aagent",
    description: "agent with inline tool in components",
    config: { kind: "aagent", system_prompt: "hi" },
    components: [
      {
        name: "my-tool",
        id: "acme/mytool@1.0.0",
        kind: "atool",
        config: { kind: "atool", binary: "bin/x" },
      },
    ],
  };
  const result = validate(manifest);
  assert.equal(result.ok, false, "should reject inline binary tool in components");
});

test("validation accepts ref to tool inside aagent components", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/agent-with-tool-ref",
    version: "0.1.0",
    kind: "aagent",
    description: "agent that refs a tool",
    config: { kind: "aagent", system_prompt: "hi" },
    components: [{ ref: "acme/some-tool@1.0.0" }],
  };
  const result = validate(manifest);
  assert.equal(result.ok, true, "should accept ref to tool in components: " + JSON.stringify(result));
});

test("validation rejects missing required fields", async () => {
  const a = new Agent("acme/bad", "0.1.0"); // no description, no system_prompt
  await assert.rejects(() => a.pack("/tmp/should-not-write.aagent"));
});

test("signature block is accepted by schema", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/signed",
    version: "1.0.0",
    kind: "aagent",
    description: "a signed agent",
    config: { kind: "aagent", system_prompt: "hi" },
    signature: {
      alg: "ed25519",
      key_fingerprint: "abc123",
      value: "base64sigvalue",
      scope: "bundle",
    },
  };
  const result = validate(manifest);
  assert.equal(result.ok, true, "signature block should be valid: " + JSON.stringify(result));
});

test("migrate v1 tool becomes mcp by default", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/mytool",
    version: "0.1.0",
    kind: "tool",
    description: "http tool",
    config: { kind: "tool", binary: "bin/x", transport: "http" },
  };
  const { manifest, errors } = migrateManifest(v1);
  assert.deepEqual(errors, []);
  assert.equal(manifest.kind, "mcp");
  const cfg = manifest.config as Record<string, unknown>;
  assert.equal(cfg.kind, "mcp");
});

test("migrate v1 grpc tool becomes atool", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/mytool",
    version: "0.1.0",
    kind: "tool",
    description: "grpc tool",
    config: { kind: "tool", binary: "bin/x", transport: "grpc" },
  };
  const { manifest, errors } = migrateManifest(v1);
  assert.deepEqual(errors, []);
  assert.equal(manifest.kind, "atool");
  const cfg = manifest.config as Record<string, unknown>;
  assert.equal(cfg.kind, "atool");
});

test("migrate v1 agent keeps config.model", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/myagent",
    version: "0.1.0",
    kind: "agent",
    description: "test agent",
    config: { kind: "agent", system_prompt: "hello", model: "claude-opus-4-7" },
  };
  const { manifest, errors } = migrateManifest(v1);
  assert.deepEqual(errors, []);
  assert.equal(manifest.schema_version, "2");
  assert.equal(manifest.kind, "aagent");
  const cfg = manifest.config as Record<string, unknown>;
  assert.equal(cfg.kind, "aagent");
  assert.equal(cfg.model, "claude-opus-4-7");
  assert.ok(!("llm" in cfg));
});

test("migrate intermediate llm field folds back to model", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/myagent",
    version: "0.1.0",
    kind: "agent",
    description: "test agent",
    config: { kind: "agent", system_prompt: "hello", llm: "claude-opus-4-7" },
  };
  const { manifest, warnings, errors } = migrateManifest(v1);
  assert.deepEqual(errors, []);
  const cfg = manifest.config as Record<string, unknown>;
  assert.equal(cfg.model, "claude-opus-4-7");
  assert.ok(!("llm" in cfg));
  assert.ok(warnings.some(w => w.includes("llm renamed to config.model")));
});

test("migrate v1 skill -> aagent, model_hint -> model, tags dropped", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/myskill",
    version: "0.2.0",
    kind: "skill",
    description: "a skill",
    config: { kind: "skill", system_prompt: "hi", model_hint: "claude-haiku", tags: ["a", "b"] },
  };
  const { manifest, warnings, errors } = migrateManifest(v1);
  assert.deepEqual(errors, []);
  assert.equal(manifest.kind, "aagent");
  const cfg = manifest.config as Record<string, unknown>;
  assert.equal(cfg.kind, "aagent");
  assert.equal(cfg.model, "claude-haiku");
  assert.ok(!("model_hint" in cfg));
  assert.ok(!("tags" in cfg));
  assert.ok(warnings.some(w => w.includes("tags removed")));
});

test("migrate v1 bundle converts to aagent with components refs", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/mybundle",
    version: "0.1.0",
    kind: "bundle",
    description: "a bundle",
    config: { kind: "bundle", components: ["acme/foo@1.0.0", "acme/bar@2.0.0"] },
  };
  const { manifest, warnings, errors } = migrateManifest(v1);
  assert.deepEqual(errors, []);
  assert.equal(manifest.kind, "aagent");
  assert.deepEqual(manifest.components, [
    { ref: "acme/foo@1.0.0" },
    { ref: "acme/bar@2.0.0" },
  ]);
  assert.ok(warnings.some(w => w.includes("bundle converted")));
});

test("migrate llm-runtime errors (no v2 equivalent)", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/myruntime",
    version: "0.1.0",
    kind: "llm-runtime",
    description: "a runtime",
    config: { kind: "llm-runtime" },
  };
  const { errors } = migrateManifest(v1);
  assert.equal(errors.length, 1);
  assert.ok(errors[0].includes("llm-runtime"));
});

test("migrate llm-backend errors (no v2 equivalent)", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/backend",
    version: "0.1.0",
    kind: "llm-backend",
    description: "a backend",
    config: { kind: "llm-backend" },
  };
  const { errors } = migrateManifest(v1);
  assert.equal(errors.length, 1);
  assert.ok(errors[0].includes("llm-backend"));
});

test("migrate strips stray signing fields with warning", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/signed",
    version: "0.1.0",
    kind: "agent",
    description: "old signed agent",
    config: { kind: "agent", system_prompt: "hi" },
    signed_at: "2024-01-01",
    key_fingerprint: "abc",
  };
  const { manifest, warnings } = migrateManifest(v1);
  assert.ok(!("signed_at" in manifest));
  assert.ok(!("key_fingerprint" in manifest));
  assert.ok(warnings.some(w => w.includes("signing fields removed")));
});
