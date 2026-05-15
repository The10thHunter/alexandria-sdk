import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import Ajv2020, { type AnySchema, type ValidateFunction } from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
import type { Manifest } from "./types.js";

const here = dirname(fileURLToPath(import.meta.url));
// Schema is copied next to dist/ at build time; in dev it lives two dirs up.
const candidates = [
  resolve(here, "atool.schema.json"),
  resolve(here, "../../../schemas/atool.schema.json"),
  resolve(here, "../../schemas/atool.schema.json"),
];

function loadSchema(): unknown {
  for (const p of candidates) {
    try {
      return JSON.parse(readFileSync(p, "utf8"));
    } catch {}
  }
  throw new Error("atool.schema.json not found near " + here);
}

let _validate: ValidateFunction | null = null;
function validator(): ValidateFunction {
  if (_validate) return _validate;
  const ajv = new Ajv2020({ allErrors: true, strict: false });
  addFormats(ajv);
  _validate = ajv.compile(loadSchema() as AnySchema);
  return _validate;
}

export interface ValidationError {
  path: string;
  message: string;
}

export function validate(manifest: unknown): { ok: true; manifest: Manifest } | { ok: false; errors: ValidationError[] } {
  const v = validator();
  if (v(manifest)) return { ok: true, manifest: manifest as Manifest };
  const errors: ValidationError[] = (v.errors ?? []).map(e => ({
    path: e.instancePath || "(root)",
    message: `${e.message ?? "invalid"}${e.params ? " " + JSON.stringify(e.params) : ""}`,
  }));
  return { ok: false, errors };
}

export function assertValid(manifest: unknown): Manifest {
  const r = validate(manifest);
  if (!r.ok) {
    const msg = r.errors.map(e => `  ${e.path}: ${e.message}`).join("\n");
    throw new Error("Invalid atool manifest:\n" + msg);
  }
  return r.manifest;
}
