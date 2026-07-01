import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Agent } from "../src/index.js";
import { publish } from "../src/publish.js";

/** Build a real packed .aagent fixture so publish()'s verify() step passes. */
async function fixture(): Promise<string> {
  const dir = mkdtempSync(join(tmpdir(), "sdk-pub-"));
  const out = join(dir, "doer-0.1.0.aagent");
  await new Agent("essentials/doer", "0.1.0")
    .description("doer")
    .systemPrompt("You are the doer.")
    .model("claude-opus-4-7")
    .pack(out);
  return out;
}

test("publish derives artifact_type from kind and POSTs the tarball to /v1/submit", async () => {
  const out = await fixture();
  const original = globalThis.fetch;
  let captured: { url: string; artifactType: unknown; hasTarball: boolean; auth: string | null } | undefined;
  try {
    globalThis.fetch = (async (url: string | URL, init?: RequestInit) => {
      const form = init!.body as FormData;
      captured = {
        url: String(url),
        artifactType: form.get("artifact_type"),
        hasTarball: form.get("tarball") instanceof Blob,
        auth: new Headers(init!.headers).get("Authorization"),
      };
      return new Response(JSON.stringify({ assessment_id: "abc" }), { status: 202 });
    }) as typeof fetch;

    const r = await publish(out, { registry: "https://reg.example/", token: "sekret" });

    assert.equal(r.ok, true);
    assert.equal(r.status, 202);
    assert.equal(r.artifactType, "aagent");
    assert.equal(captured!.url, "https://reg.example/v1/submit"); // trailing slash trimmed
    assert.equal(captured!.artifactType, "aagent"); // derived from manifest kind
    assert.equal(captured!.hasTarball, true);
    assert.equal(captured!.auth, "Bearer sekret");
  } finally {
    globalThis.fetch = original;
  }
});

test("publish surfaces a non-2xx registry response as ok=false", async () => {
  const out = await fixture();
  const original = globalThis.fetch;
  try {
    globalThis.fetch = (async () =>
      new Response(JSON.stringify({ error: "stage1_kind_enum" }), { status: 400 })) as typeof fetch;
    const r = await publish(out, { registry: "https://reg.example" });
    assert.equal(r.ok, false);
    assert.equal(r.status, 400);
  } finally {
    globalThis.fetch = original;
  }
});

test("publish honors an explicit artifact_type override", async () => {
  const out = await fixture();
  const original = globalThis.fetch;
  let sentType: unknown;
  try {
    globalThis.fetch = (async (_url: string | URL, init?: RequestInit) => {
      sentType = (init!.body as FormData).get("artifact_type");
      return new Response("{}", { status: 202 });
    }) as typeof fetch;
    await publish(out, { registry: "https://reg.example", artifactType: "amodel:llm-backend" });
    assert.equal(sentType, "amodel:llm-backend");
  } finally {
    globalThis.fetch = original;
  }
});
