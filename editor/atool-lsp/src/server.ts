#!/usr/bin/env node
/**
 * Language Server for atool.json. Provides:
 *   - Schema-driven JSON diagnostics
 *   - Completions for `kind`, `transport`, `config.kind`, top-level keys
 *   - Hovers for known fields
 *
 * Works in any LSP client. The companion VSCode extension wires it up
 * automatically; for Neovim see editor/nvim/README.md.
 */
import {
  createConnection,
  TextDocuments,
  ProposedFeatures,
  DiagnosticSeverity,
  TextDocumentSyncKind,
  CompletionItem,
  CompletionItemKind,
  Hover,
  MarkupKind,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import Ajv, { ErrorObject } from "ajv";
import addFormats from "ajv-formats";
import * as fs from "node:fs";
import * as path from "node:path";

const connection = createConnection(ProposedFeatures.all);
const documents = new TextDocuments(TextDocument);

function findSchema(): any {
  const candidates = [
    path.resolve(__dirname, "atool.schema.json"),
    path.resolve(__dirname, "../../../schemas/atool.schema.json"),
    path.resolve(__dirname, "../../schemas/atool.schema.json"),
    path.resolve(process.cwd(), "schemas/atool.schema.json"),
  ];
  for (const p of candidates) {
    try { return JSON.parse(fs.readFileSync(p, "utf8")); } catch {}
  }
  throw new Error("atool.schema.json not found");
}

const ajv = new Ajv({ allErrors: true, strict: false });
addFormats(ajv);
const validate = ajv.compile(findSchema());

const TOP_LEVEL_KEYS: Record<string, string> = {
  schema_version: "Always \"2\".",
  name: "'vendor/name' or 'name'.",
  version: "SemVer (e.g. 0.1.0).",
  kind: "tool | agent | skill.",
  description: "One-line description.",
  author: "Optional author string.",
  license: "Optional license (e.g. Apache-2.0).",
  requires_alexandria: "Optional minimum Alexandria version.",
  dependencies: "Array of { name, version }.",
  files: "Array of { archive_path, install_path, executable?, sha256? }.",
  permissions: "{ provides_tools?, needs_tools?, suggested_role? }.",
  config: "Per-kind config object (its inner `kind` must match the top-level `kind`).",
  components: "Array of ComponentItem (only valid on kind=agent). Each item is either { ref: 'ns/name@version' } or an inline sub-agent/skill.",
  install: "Install-time merge rules (only on agents with non-empty components[]). Contains flatten block.",
  signature: "Cryptographic signature block (EE-signed archives). { alg, key_fingerprint, value, scope }.",
};

// v2: only tool, agent, skill
const KIND_VALUES = ["tool", "agent", "skill"];
const TRANSPORT_VALUES = ["http", "sse"];
const K8S_TRANSPORT_VALUES = ["grpc", "http", "sse"];
// Only agent and skill may be inline components — tools must be refs
const COMPONENT_KIND_VALUES = ["agent", "skill"];
const SYSTEM_PROMPT_FLATTEN_VALUES = ["concat", "root_wins", "error_on_conflict"];
const ALLOWED_TOOLS_FLATTEN_VALUES = ["union", "root_wins"];
const SIGNATURE_SCOPE_VALUES = ["bundle", "per_component"];

connection.onInitialize(() => ({
  capabilities: {
    textDocumentSync: TextDocumentSyncKind.Incremental,
    completionProvider: { triggerCharacters: ["\"", ":", " "] },
    hoverProvider: true,
  },
}));

documents.onDidChangeContent(change => validateDoc(change.document));
documents.onDidOpen(change => validateDoc(change.document));

function validateDoc(doc: TextDocument): void {
  const text = doc.getText();
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (e: any) {
    connection.sendDiagnostics({
      uri: doc.uri,
      diagnostics: [{
        severity: DiagnosticSeverity.Error,
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
        message: `JSON parse error: ${e.message ?? String(e)}`,
        source: "atool-lsp",
      }],
    });
    return;
  }

  const diagnostics = [];

  // Schema validation errors
  const ok = validate(parsed);
  const schemaErrors = ok ? [] : (validate.errors ?? []).map((e: ErrorObject) => ({
    severity: DiagnosticSeverity.Error,
    range: rangeForPath(text, e.instancePath),
    message: `${e.instancePath || "(root)"}: ${e.message ?? "invalid"}` +
      (e.params && Object.keys(e.params).length ? ` ${JSON.stringify(e.params)}` : ""),
    source: "atool-lsp",
  }));
  diagnostics.push(...schemaErrors);

  // Custom semantic warnings beyond what JSON Schema can express
  if (typeof parsed === "object" && parsed !== null) {
    const m = parsed as Record<string, unknown>;

    // Warn if components present on non-agent kind
    if (m.kind !== "agent" && m.components !== undefined) {
      diagnostics.push({
        severity: DiagnosticSeverity.Warning,
        range: rangeForPath(text, "/components"),
        message: "`components` is only valid on kind=agent",
        source: "atool-lsp",
      });
    }

    // Warn if llm present on tool kind
    if (m.kind === "tool") {
      const cfg = m.config as Record<string, unknown> | undefined;
      if (cfg && cfg.llm !== undefined) {
        diagnostics.push({
          severity: DiagnosticSeverity.Warning,
          range: rangeForPath(text, "/config/llm"),
          message: "`llm` is not meaningful on kind=tool",
          source: "atool-lsp",
        });
      }
    }

    // Warn if schema_version is still "1"
    if (m.schema_version === "1") {
      diagnostics.push({
        severity: DiagnosticSeverity.Warning,
        range: rangeForPath(text, "/schema_version"),
        message: "schema_version \"1\" is deprecated; run `alex-sdk migrate` to upgrade to v2",
        source: "atool-lsp",
      });
    }
  }

  connection.sendDiagnostics({ uri: doc.uri, diagnostics });
}

/** Best-effort range for a JSON pointer-ish instancePath. Highlights the key. */
function rangeForPath(text: string, instancePath: string) {
  if (!instancePath) {
    return { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } };
  }
  const segments = instancePath.split("/").filter(Boolean);
  const last = segments[segments.length - 1];
  const needle = `"${last}"`;
  const idx = text.indexOf(needle);
  if (idx < 0) return { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } };
  const before = text.slice(0, idx);
  const line = before.split("\n").length - 1;
  const character = idx - (before.lastIndexOf("\n") + 1);
  return { start: { line, character }, end: { line, character: character + needle.length } };
}

connection.onCompletion((params): CompletionItem[] => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return [];
  const offset = doc.offsetAt(params.position);
  const text = doc.getText();
  const ctx = text.slice(Math.max(0, offset - 120), offset);

  // Top-level kind values
  if (/"kind"\s*:\s*"?[a-z-]*$/.test(ctx) && !/"components"/.test(ctx.slice(-40))) {
    return KIND_VALUES.map(v => ({ label: v, kind: CompletionItemKind.EnumMember }));
  }

  // Component kind values (only agent and skill, not tool)
  if (/"components"[\s\S]*"kind"\s*:\s*"?[a-z-]*$/.test(ctx)) {
    return COMPONENT_KIND_VALUES.map(v => ({ label: v, kind: CompletionItemKind.EnumMember }));
  }

  // Transport values
  if (/"transport"\s*:\s*"?[a-z]*$/.test(ctx) && !/"k8s_transport"/.test(ctx)) {
    return TRANSPORT_VALUES.map(v => ({ label: v, kind: CompletionItemKind.EnumMember }));
  }
  if (/"k8s_transport"\s*:\s*"?[a-z]*$/.test(ctx)) {
    return K8S_TRANSPORT_VALUES.map(v => ({ label: v, kind: CompletionItemKind.EnumMember }));
  }

  // install.flatten.system_prompt
  if (/"system_prompt"\s*:\s*"?[a-z_]*$/.test(ctx) && /"flatten"/.test(ctx)) {
    return SYSTEM_PROMPT_FLATTEN_VALUES.map(v => ({ label: v, kind: CompletionItemKind.EnumMember }));
  }

  // install.flatten.allowed_tools
  if (/"allowed_tools"\s*:\s*"?[a-z_]*$/.test(ctx) && /"flatten"/.test(ctx)) {
    return ALLOWED_TOOLS_FLATTEN_VALUES.map(v => ({ label: v, kind: CompletionItemKind.EnumMember }));
  }

  // signature.scope
  if (/"scope"\s*:\s*"?[a-z_]*$/.test(ctx)) {
    return SIGNATURE_SCOPE_VALUES.map(v => ({ label: v, kind: CompletionItemKind.EnumMember }));
  }

  return Object.keys(TOP_LEVEL_KEYS).map(k => ({
    label: k,
    kind: CompletionItemKind.Property,
    detail: TOP_LEVEL_KEYS[k],
  }));
});

connection.onHover((params): Hover | null => {
  const doc = documents.get(params.textDocument.uri);
  if (!doc) return null;
  const text = doc.getText();
  const offset = doc.offsetAt(params.position);
  const left = text.slice(0, offset).match(/"([a-z_]+)"?$/);
  const right = text.slice(offset).match(/^([a-z_]*)"/);
  const word = (left?.[1] ?? "") + (right?.[1] ?? "");
  const doc_ = TOP_LEVEL_KEYS[word];
  if (!doc_) return null;
  return { contents: { kind: MarkupKind.Markdown, value: `**${word}** — ${doc_}` } };
});

documents.listen(connection);
connection.listen();
