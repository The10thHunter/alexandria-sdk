package alexsdk_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"regexp"
	"testing"

	alexsdk "github.com/The10thHunter/alexandria-sdk/packages/go"
)

func TestAgentBuildPackVerifyRoundTrip(t *testing.T) {
	dir := t.TempDir()
	out := filepath.Join(dir, "demo-agent-0.1.0.aagent")

	a := alexsdk.NewAgent("acme/demo-agent", "0.1.0").
		Description("A demo agent for the round-trip test").
		Author("acme").
		License("MIT").
		SystemPrompt("You are a friendly demo agent.").
		AllowedTools([]string{"acme/echo"}).
		LLM("claude-opus-4")

	m, err := a.Pack(out)
	if err != nil {
		t.Fatalf("Pack: %v", err)
	}
	if m.Name != "acme/demo-agent" || m.Version != "0.1.0" {
		t.Fatalf("unexpected manifest: %+v", m)
	}
	if m.SchemaVersion != "2" {
		t.Fatalf("expected schema_version=2, got %q", m.SchemaVersion)
	}

	got, err := alexsdk.Verify(out)
	if err != nil {
		t.Fatalf("Verify: %v", err)
	}
	if got.Kind != alexsdk.KindAgent {
		t.Fatalf("expected kind=agent, got %q", got.Kind)
	}
	ac, err := got.AgentConfig()
	if err != nil {
		t.Fatalf("AgentConfig: %v", err)
	}
	if ac.SystemPrompt == "" {
		t.Fatal("expected non-empty system_prompt")
	}
	if ac.LLM != "claude-opus-4" {
		t.Fatalf("expected llm=claude-opus-4, got %q", ac.LLM)
	}
}

func TestToolStagedBinaryHasSHA256(t *testing.T) {
	dir := t.TempDir()
	binPath := filepath.Join(dir, "echo-bin")
	if err := os.WriteFile(binPath, []byte("#!/bin/sh\necho hi\n"), 0o755); err != nil {
		t.Fatal(err)
	}

	out := filepath.Join(dir, "echo-0.1.0.atool")
	tool := alexsdk.NewTool("acme/echo", "0.1.0").
		Description("Tool with a staged binary").
		Binary("bin/echo").
		Port(8080).
		Transport("http").
		StageFile(binPath, alexsdk.FileEntry{
			ArchivePath: "bin/echo",
			InstallPath: "bin/echo",
			Executable:  true,
		})

	m, err := tool.Pack(out)
	if err != nil {
		t.Fatalf("Pack: %v", err)
	}
	if len(m.Files) != 1 {
		t.Fatalf("expected 1 file, got %d", len(m.Files))
	}
	hexRe := regexp.MustCompile(`^[a-f0-9]{64}$`)
	if !hexRe.MatchString(m.Files[0].SHA256) {
		t.Fatalf("expected 64-char hex sha256, got %q", m.Files[0].SHA256)
	}
	if _, err := alexsdk.Verify(out); err != nil {
		t.Fatalf("Verify: %v", err)
	}
}

func TestAgentWithInlineSubAgent(t *testing.T) {
	dir := t.TempDir()

	child := alexsdk.NewAgent("acme/child", "0.1.0").
		Description("child agent").
		SystemPrompt("You are a child agent.")

	parent := alexsdk.NewAgent("acme/parent", "0.1.0").
		Description("parent agent with component").
		SystemPrompt("You orchestrate sub-agents.").
		Component("child-agent", "acme/child@0.1.0", child)

	out := filepath.Join(dir, "parent-0.1.0.aagent")
	m, err := parent.Pack(out)
	if err != nil {
		t.Fatalf("Pack parent: %v", err)
	}
	if len(m.Components) != 1 {
		t.Fatalf("expected 1 component, got %d", len(m.Components))
	}
	comp := m.Components[0]
	if comp.Name != "child-agent" {
		t.Fatalf("expected component name=child-agent, got %q", comp.Name)
	}
	if comp.ID != "acme/child@0.1.0" {
		t.Fatalf("expected component id=acme/child@0.1.0, got %q", comp.ID)
	}
	if _, err := alexsdk.Verify(out); err != nil {
		t.Fatalf("Verify parent: %v", err)
	}
}

func TestAgentWithRefComponents(t *testing.T) {
	dir := t.TempDir()

	agent := alexsdk.NewAgent("acme/orchestrator", "1.0.0").
		Description("orchestrator").
		SystemPrompt("You use tools.").
		Ref("acme/some-tool@1.0.0").
		Ref("acme/some-agent@2.0.0")

	out := filepath.Join(dir, "orchestrator-1.0.0.aagent")
	m, err := agent.Pack(out)
	if err != nil {
		t.Fatalf("Pack: %v", err)
	}
	if len(m.Components) != 2 {
		t.Fatalf("expected 2 components, got %d", len(m.Components))
	}
	if m.Components[0].Ref != "acme/some-tool@1.0.0" {
		t.Fatalf("expected ref=acme/some-tool@1.0.0, got %q", m.Components[0].Ref)
	}
}

func TestAgentWithFlattenRules(t *testing.T) {
	dir := t.TempDir()

	agent := alexsdk.NewAgent("acme/flat", "0.1.0").
		Description("agent with flatten").
		SystemPrompt("You merge sub-agents.").
		Ref("acme/sub@1.0.0").
		Flatten(alexsdk.InstallFlatten{
			SystemPrompt: "concat",
			AllowedTools: "union",
		})

	out := filepath.Join(dir, "flat-0.1.0.aagent")
	m, err := agent.Pack(out)
	if err != nil {
		t.Fatalf("Pack: %v", err)
	}
	if m.Install == nil || m.Install.Flatten == nil {
		t.Fatal("expected install.flatten to be set")
	}
	if m.Install.Flatten.SystemPrompt != "concat" {
		t.Fatalf("expected system_prompt=concat, got %q", m.Install.Flatten.SystemPrompt)
	}
}

func TestValidationRejectsComponentsOnTool(t *testing.T) {
	// Build a manifest dict that has components on a tool — schema should reject it.
	raw := map[string]interface{}{
		"schema_version": "2",
		"name":           "acme/bad-tool",
		"version":        "0.1.0",
		"kind":           "tool",
		"description":    "tool with components",
		"config":         map[string]interface{}{"kind": "tool", "binary": "bin/x"},
		"components":     []interface{}{map[string]interface{}{"ref": "acme/foo@1.0.0"}},
	}
	b, _ := json.Marshal(raw)
	var m alexsdk.Manifest
	_ = json.Unmarshal(b, &m)
	if err := alexsdk.AssertValid(&m); err == nil {
		t.Fatal("expected validation error for components on tool")
	}
}

func TestValidationAcceptsRefToToolInAgentComponents(t *testing.T) {
	raw := map[string]interface{}{
		"schema_version": "2",
		"name":           "acme/agent-with-tool-ref",
		"version":        "0.1.0",
		"kind":           "agent",
		"description":    "agent that refs a tool",
		"config":         map[string]interface{}{"kind": "agent", "system_prompt": "hi"},
		"components":     []interface{}{map[string]interface{}{"ref": "acme/some-tool@1.0.0"}},
	}
	b, _ := json.Marshal(raw)
	var m alexsdk.Manifest
	if err := json.Unmarshal(b, &m); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if err := alexsdk.AssertValid(&m); err != nil {
		t.Fatalf("expected valid manifest, got error: %v", err)
	}
}

func TestSkillBuilderLLMField(t *testing.T) {
	dir := t.TempDir()
	out := filepath.Join(dir, "skill-0.1.0.atool")

	skill := alexsdk.NewSkill("acme/myskill", "0.1.0").
		Description("a skill with llm hint").
		SystemPrompt("You are specialized.").
		LLM("claude-haiku").
		Tags([]string{"research"})

	m, err := skill.Pack(out)
	if err != nil {
		t.Fatalf("Pack: %v", err)
	}
	sc, err := m.SkillConfig()
	if err != nil {
		t.Fatalf("SkillConfig: %v", err)
	}
	if sc.LLM != "claude-haiku" {
		t.Fatalf("expected llm=claude-haiku, got %q", sc.LLM)
	}
}

func TestInvalidManifestMissingDescription(t *testing.T) {
	a := alexsdk.NewAgent("acme/no-desc", "0.1.0").
		SystemPrompt("hello")
	if _, err := a.Build(); err == nil {
		t.Fatal("expected validation error for missing description")
	}
}

// TestMigrateV1BundleToAgent tests that migrate converts bundle→agent.
// This is a unit test of the migrate logic exposed via the Go CLI.
// The actual CLI is tested via the migrate command.
func TestMigrateManifest(t *testing.T) {
	v1 := map[string]interface{}{
		"schema_version": "1",
		"name":           "acme/mybundle",
		"version":        "0.1.0",
		"kind":           "bundle",
		"description":    "a bundle",
		"config": map[string]interface{}{
			"kind":       "bundle",
			"components": []interface{}{"acme/foo@1.0.0", "acme/bar@2.0.0"},
		},
	}
	result, warnings, errors := alexsdk.MigrateManifest(v1)
	if len(errors) > 0 {
		t.Fatalf("unexpected errors: %v", errors)
	}
	if result["kind"] != "agent" {
		t.Fatalf("expected kind=agent, got %v", result["kind"])
	}
	if len(warnings) == 0 {
		t.Fatal("expected at least one warning")
	}
	comps, ok := result["components"].([]interface{})
	if !ok || len(comps) != 2 {
		t.Fatalf("expected 2 components, got %v", result["components"])
	}
}

func TestMigrateLLMRuntimeErrors(t *testing.T) {
	v1 := map[string]interface{}{
		"schema_version": "1",
		"name":           "acme/runtime",
		"version":        "0.1.0",
		"kind":           "llm-runtime",
		"description":    "a runtime",
		"config":         map[string]interface{}{"kind": "llm-runtime"},
	}
	_, _, errors := alexsdk.MigrateManifest(v1)
	if len(errors) == 0 {
		t.Fatal("expected error for llm-runtime kind")
	}
}
