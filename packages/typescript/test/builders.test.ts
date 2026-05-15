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
    .llm("claude-opus-4-7")
    .historyLimit(50)
    .pack(out);

  const m = await verify(out);
  assert.equal(m.kind, "agent");
  assert.equal(m.name, "acme/research");
  assert.equal(m.schema_version, "2");
  if (m.config.kind === "agent") assert.equal(m.config.llm, "claude-opus-4-7");
});

test("Tool builder hashes a staged binary", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const src = join(dir, "src");
  mkdirSync(join(src, "bin"), { recursive: true });
  writeFileSync(join(src, "bin", "mytool"), "#!/bin/sh\nexit 0\n");

  const out = join(dir, "mytool-0.1.0.atool");
  await new Tool("acme/mytool", "0.1.0")
    .description("daemon")
    .binary("bin/mytool").port(7800).transport("http")
    .file({ archive_path: "bin/mytool", install_path: "tools/mytool/bin/mytool", executable: true })
    .pack(out, { srcDir: src });

  const r = await inspect(out);
  assert.equal(r.manifest.kind, "tool");
  assert.equal(r.manifest.schema_version, "2");
  const entry = r.manifest.files!.find(f => f.archive_path === "bin/mytool")!;
  assert.match(entry.sha256!, /^[a-f0-9]{64}$/);
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
  assert.equal(m.kind, "agent");
  assert.ok(m.components, "components should be present");
  assert.equal(m.components!.length, 1);
  const comp = m.components![0];
  assert.ok("name" in comp && comp.name === "child-agent", "inline component has name");
  assert.ok("id" in comp && comp.id === "acme/child@0.1.0", "inline component has id");
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

test("validation rejects components on tool kind", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/bad-tool",
    version: "0.1.0",
    kind: "tool",
    description: "a tool that wrongly has components",
    config: { kind: "tool", binary: "bin/x" },
    components: [{ ref: "acme/foo@1.0" }],
  };
  const result = validate(manifest);
  assert.equal(result.ok, false, "should reject components on tool");
});

test("validation rejects inline tool in components", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/bad-agent",
    version: "0.1.0",
    kind: "agent",
    description: "agent with inline tool in components",
    config: { kind: "agent", system_prompt: "hi" },
    components: [
      {
        name: "my-tool",
        id: "acme/mytool@1.0.0",
        kind: "tool",
        config: { kind: "tool", binary: "bin/x" },
      },
    ],
  };
  const result = validate(manifest);
  assert.equal(result.ok, false, "should reject inline tool in components");
});

test("validation accepts ref to tool inside agent components", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/agent-with-tool-ref",
    version: "0.1.0",
    kind: "agent",
    description: "agent that refs a tool",
    config: { kind: "agent", system_prompt: "hi" },
    components: [{ ref: "acme/some-tool@1.0.0" }],
  };
  const result = validate(manifest);
  assert.equal(result.ok, true, "should accept ref to tool in components: " + JSON.stringify(result));
});

test("validation rejects missing required fields", async () => {
  const a = new Agent("acme/bad", "0.1.0"); // no description, no system_prompt
  await assert.rejects(() => a.pack("/tmp/should-not-write.aagent"));
});

test("Skill builder with llm field round-trips", async () => {
  const dir = mkdtempSync(join(tmpdir(), "sdk-"));
  const out = join(dir, "skill-0.1.0.atool");

  await new Skill("acme/my-skill", "0.1.0")
    .description("a skill with llm hint")
    .systemPrompt("You are a specialized skill.")
    .llm("claude-haiku")
    .tags(["research", "writing"])
    .pack(out);

  const m = await verify(out);
  assert.equal(m.kind, "skill");
  assert.equal(m.schema_version, "2");
  if (m.config.kind === "skill") {
    assert.equal(m.config.llm, "claude-haiku");
    assert.deepEqual(m.config.tags, ["research", "writing"]);
  }
});

test("signature block is accepted by schema", () => {
  const manifest = {
    schema_version: "2",
    name: "acme/signed",
    version: "1.0.0",
    kind: "agent",
    description: "a signed agent",
    config: { kind: "agent", system_prompt: "hi" },
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

test("migrate v1 agent renames model to llm", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/myagent",
    version: "0.1.0",
    kind: "agent",
    description: "test agent",
    config: { kind: "agent", system_prompt: "hello", model: "claude-opus-4-7" },
  };
  const { manifest, warnings, errors } = migrateManifest(v1);
  assert.deepEqual(errors, []);
  assert.equal(manifest.schema_version, "2");
  const cfg = manifest.config as Record<string, unknown>;
  assert.equal(cfg.llm, "claude-opus-4-7");
  assert.ok(!("model" in cfg));
  assert.ok(warnings.some(w => w.includes("model renamed")));
});

test("migrate v1 skill renames model_hint to llm", () => {
  const v1 = {
    schema_version: "1",
    name: "acme/myskill",
    version: "0.2.0",
    kind: "skill",
    description: "a skill",
    config: { kind: "skill", system_prompt: "hi", model_hint: "claude-haiku" },
  };
  const { manifest, errors } = migrateManifest(v1);
  assert.deepEqual(errors, []);
  const cfg = manifest.config as Record<string, unknown>;
  assert.equal(cfg.llm, "claude-haiku");
  assert.ok(!("model_hint" in cfg));
});

test("migrate v1 bundle converts to agent with components refs", () => {
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
  assert.equal(manifest.kind, "agent");
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
