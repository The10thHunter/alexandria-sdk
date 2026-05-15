import { createReadStream, createWriteStream, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { open as fsOpen, readFile } from "node:fs/promises";
import { createGzip, createGunzip } from "node:zlib";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { Readable, pipeline } from "node:stream";
import { promisify } from "node:util";
import { extract as tarExtract, pack as tarPack } from "tar-stream";
import { assertValid } from "./schema.js";
import type { Manifest } from "./types.js";

const pipe = promisify(pipeline);

async function sha256File(p: string): Promise<string> {
  const h = createHash("sha256");
  await pipe(createReadStream(p), h);
  return h.digest("hex");
}

/**
 * Pack a source directory into a gzipped tar `.atool`/`.aagent`.
 * Mirrors `alex_package::pack`:
 *   1. read `<srcDir>/atool.json`
 *   2. for each `files[]`, compute sha256 of the file at `archive_path` and
 *      write it back into the manifest entry
 *   3. emit `atool.json` first, then each declared file, gzipped.
 */
export async function pack(srcDir: string, outPath: string): Promise<Manifest> {
  const manifestPath = join(srcDir, "atool.json");
  const manifest: Manifest = JSON.parse(readFileSync(manifestPath, "utf8"));

  if (manifest.files) {
    for (const f of manifest.files) {
      const abs = resolve(srcDir, f.archive_path);
      f.sha256 = await sha256File(abs);
    }
  }
  assertValid(manifest);

  const tar = tarPack();
  const manifestBytes = Buffer.from(JSON.stringify(manifest, null, 2));
  tar.entry({ name: "atool.json", size: manifestBytes.length, mode: 0o644 }, manifestBytes);

  for (const f of manifest.files ?? []) {
    const abs = resolve(srcDir, f.archive_path);
    const st = statSync(abs);
    const mode = f.executable ? 0o755 : 0o644;
    const entry = tar.entry({ name: f.archive_path, size: st.size, mode });
    await pipe(createReadStream(abs), entry);
  }
  tar.finalize();

  await pipe(tar as unknown as Readable, createGzip(), createWriteStream(outPath));
  return manifest;
}

/**
 * Verify a `.atool`/`.aagent`: extract in-memory, parse the manifest, re-hash
 * every declared file with a non-empty sha256.
 */
export async function verify(pkgPath: string): Promise<Manifest> {
  const { manifest, fileBytes } = await readArchive(pkgPath);
  assertValid(manifest);

  for (const f of manifest.files ?? []) {
    if (!f.sha256) continue;
    const buf = fileBytes.get(f.archive_path);
    if (!buf) throw new Error(`declared file missing from archive: ${f.archive_path}`);
    const got = createHash("sha256").update(buf).digest("hex");
    if (got !== f.sha256) {
      throw new Error(`sha256 mismatch for ${f.archive_path}: want ${f.sha256}, got ${got}`);
    }
  }
  return manifest;
}

export interface InspectResult {
  manifest: Manifest;
  files: Array<{ name: string; size: number }>;
  totalBytes: number;
}

export async function inspect(pkgPath: string): Promise<InspectResult> {
  const { manifest, sizes } = await readArchive(pkgPath, { keepBytes: false });
  const files = [...sizes.entries()].map(([name, size]) => ({ name, size }));
  const totalBytes = files.reduce((a, b) => a + b.size, 0);
  return { manifest, files, totalBytes };
}

async function readArchive(pkgPath: string, opts: { keepBytes?: boolean } = { keepBytes: true }): Promise<{ manifest: Manifest; fileBytes: Map<string, Buffer>; sizes: Map<string, number> }> {
  const fd = await fsOpen(pkgPath, "r");
  const stream = fd.createReadStream();
  const ex = tarExtract();
  let manifest: Manifest | null = null;
  const fileBytes = new Map<string, Buffer>();
  const sizes = new Map<string, number>();

  ex.on("entry", (header, str, next) => {
    const chunks: Buffer[] = [];
    str.on("data", (c: Buffer) => chunks.push(c));
    str.on("end", () => {
      const buf = Buffer.concat(chunks);
      sizes.set(header.name, buf.length);
      if (header.name === "atool.json") {
        manifest = JSON.parse(buf.toString("utf8"));
      } else if (opts.keepBytes !== false) {
        fileBytes.set(header.name, buf);
      }
      next();
    });
    str.resume();
  });

  await pipe(stream, createGunzip(), ex);
  if (!manifest) throw new Error("atool.json not found in archive");
  return { manifest: manifest as Manifest, fileBytes, sizes };
}

export async function readManifest(srcDir: string): Promise<Manifest> {
  return JSON.parse(await readFile(join(srcDir, "atool.json"), "utf8")) as Manifest;
}

export function writeManifest(srcDir: string, manifest: Manifest): void {
  const path = join(srcDir, "atool.json");
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(manifest, null, 2) + "\n");
}
