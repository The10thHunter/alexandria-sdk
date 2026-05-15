/**
 * Programmatic build script for an agent collection. Run with:
 *   npx tsx build.ts
 *
 * Produces the collection `.aagent` in ./dist.
 *
 * In v2, Agent collections are expressed as an agent with components[].
 * Sub-agents can be inline (embedded in the manifest) or refs (published
 * separately and referenced by ns/name@version).
 */
import { Agent } from "@alexandria/sdk";
import { mkdirSync } from "node:fs";

mkdirSync("./dist", { recursive: true });

const research = new Agent("acme/research-agent", "0.1.0")
  .description("Research assistant")
  .systemPrompt("You are a senior research assistant.")
  .allowedTools(["web-search", "pdf-parser"])
  .llm("claude-opus-4-7")
  .historyLimit(100)
  .needsTools(["web-search", "pdf-parser"]);

const writer = new Agent("acme/writer-agent", "0.1.0")
  .description("Writer agent")
  .systemPrompt("You are an editor. Tighten prose, preserve voice.")
  .llm("claude-opus-4-7")
  .historyLimit(50);

// Option A: Pack each agent individually + compose via refs in the collection manifest.
// (The atool.json at root uses { "ref": "..." } references.)
await research.pack("./dist/research-agent-0.1.0.aagent");
await writer.pack("./dist/writer-agent-0.1.0.aagent");

// Option B: Embed agents inline within a parent agent's components[].
// Uncomment if you want a single self-contained archive:
//
// const collection = new Agent("acme/my-agents", "0.1.0")
//   .description("Research + writing agents")
//   .systemPrompt("You are an orchestrator that delegates to research and writing sub-agents.")
//   .component("research", "acme/research-agent@0.1.0", research)
//   .component("writer", "acme/writer-agent@0.1.0", writer)
//   .flatten({ system_prompt: "root_wins", allowed_tools: "union" });
// await collection.pack("./dist/my-agents-0.1.0.aagent");
