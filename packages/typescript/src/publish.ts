import { readFileSync } from "node:fs";
import { basename } from "node:path";
import { verify } from "./pack.js";
import type { Manifest } from "./types.js";

export interface PublishOptions {
  /** Base URL of the Alexandria registry, e.g. https://registry.alexandria.dev */
  registry: string;
  /** Optional bearer token (sent as Authorization: Bearer <token>). */
  token?: string;
  /** Override the artifact_type; defaults to the manifest's kind. */
  artifactType?: string;
}

export interface PublishResult {
  status: number;
  ok: boolean;
  body: unknown;
  name: string;
  version: string;
  artifactType: string;
}

/**
 * Publish a packed `.atool`/`.aagent` archive to an Alexandria registry via
 * `POST {registry}/v1/submit` — the missing consumer-side half of the registry
 * loop (`alexandria install` pulls; nothing pushed until now).
 *
 * The archive is re-verified (hashes + schema) before it ships, and the
 * multipart body mirrors the registry's `handleSubmit` contract exactly:
 *   - `artifact_type` — derived from the manifest kind (mcp|atool|aagent),
 *     overridable for amodel sub-variants.
 *   - `tarball` — the packed archive bytes.
 */
export async function publish(pkgPath: string, opts: PublishOptions): Promise<PublishResult> {
  // Re-hash + schema-validate the archive before publishing — never ship an
  // archive the local runtime would itself reject.
  const manifest: Manifest = await verify(pkgPath);
  const artifactType = opts.artifactType ?? manifest.kind;

  const bytes = readFileSync(pkgPath);
  const form = new FormData();
  form.set("artifact_type", artifactType);
  form.set("tarball", new Blob([new Uint8Array(bytes)], { type: "application/gzip" }), basename(pkgPath));

  const base = opts.registry.replace(/\/+$/, "");
  const headers: Record<string, string> = {};
  if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;

  const resp = await fetch(`${base}/v1/submit`, { method: "POST", body: form, headers });
  const text = await resp.text();
  let body: unknown;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }

  return {
    status: resp.status,
    ok: resp.ok,
    body,
    name: manifest.name,
    version: manifest.version,
    artifactType,
  };
}
