package alexsdk_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"regexp"
	"strings"
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
		Model("claude-opus-4")

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
	if got.Kind != alexsdk.KindAagent {
		t.Fatalf("expected kind=aagent, got %q", got.Kind)
	}
	ac, err := got.AagentConfig()
	if err != nil {
		t.Fatalf("AagentConfig: %v", err)
	}
	if ac.SystemPrompt == "" {
		t.Fatal("expected non-empty system_prompt")
	}
	if ac.Model != "claude-opus-4" {
		t.Fatalf("expected model=claude-opus-4, got %q", ac.Model)
	}
}

func TestToolDefaultsToAtool(t *testing.T) {
	dir := t.TempDir()
	binPath := filepath.Join(dir, "echo-bin")
	if err := os.WriteFile(binPath, []byte("#!/bin/sh\necho hi\n"), 0o755); err != nil {
		t.Fatal(err)
	}

	out := filepath.Join(dir, "echo-0.1.0.atool")
	tool := alexsdk.NewTool("acme/echo", "0.1.0").
		Description("Tool with a staged binary").
		Binary("bin/echo").
		InterfaceMajor(2).
		StageFile(binPath, alexsdk.FileEntry{
			ArchivePath: "bin/echo",
			InstallPath: "bin/echo",
			Executable:  true,
		})

	m, err := tool.Pack(out)
	if err != nil {
		t.Fatalf("Pack: %v", err)
	}
	if m.Kind != alexsdk.KindAtool {
		t.Fatalf("expected kind=atool, got %q", m.Kind)
	}
	ac, err := m.AtoolConfig()
	if err != nil {
		t.Fatalf("AtoolConfig: %v", err)
	}
	if ac.InterfaceMajor == nil || *ac.InterfaceMajor != 2 {
		t.Fatalf("expected interface_major=2, got %v", ac.InterfaceMajor)
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

func TestToolCredentialAndEnvRoundTrip(t *testing.T) {
	dir := t.TempDir()
	binPath := filepath.Join(dir, "gh-bin")
	if err := os.WriteFile(binPath, []byte("#!/bin/sh\nexit 0\n"), 0o755); err != nil {
		t.Fatal(err)
	}

	out := filepath.Join(dir, "github-0.1.0.atool")
	tool := alexsdk.NewTool("acme/github", "0.1.0").
		Description("GitHub tool with a declared credential").
		Binary("bin/gh").
		Credential(alexsdk.CredentialDecl{
			Env:         "GITHUB_PERSONAL_ACCESS_TOKEN",
			Required:    true,
			Description: "GitHub PAT",
		}).
		EnvVar(alexsdk.EnvDecl{Name: "GITHUB_HOST", Default: "github.com"}).
		StageFile(binPath, alexsdk.FileEntry{
			ArchivePath: "bin/gh",
			InstallPath: "tools/gh/bin/gh",
			Executable:  true,
		})

	if _, err := tool.Pack(out); err != nil {
		t.Fatalf("Pack: %v", err)
	}

	got, err := alexsdk.Verify(out)
	if err != nil {
		t.Fatalf("Verify: %v", err)
	}
	ac, err := got.AtoolConfig()
	if err != nil {
		t.Fatalf("AtoolConfig: %v", err)
	}
	if len(ac.Credentials) != 1 {
		t.Fatalf("expected 1 credential, got %d", len(ac.Credentials))
	}
	c := ac.Credentials[0]
	if c.Env != "GITHUB_PERSONAL_ACCESS_TOKEN" {
		t.Fatalf("unexpected credential env %q", c.Env)
	}
	if !c.Required {
		t.Fatal("expected credential required=true")
	}
	if c.Secret == nil || *c.Secret != true {
		t.Fatalf("expected secret to default to true, got %v", c.Secret)
	}
	if c.Rotation != "respawn" {
		t.Fatalf("expected rotation to default to respawn, got %q", c.Rotation)
	}
	if c.Description != "GitHub PAT" {
		t.Fatalf("unexpected description %q", c.Description)
	}
	if len(ac.Env) != 1 || ac.Env[0].Name != "GITHUB_HOST" || ac.Env[0].Default != "github.com" {
		t.Fatalf("unexpected env decls %+v", ac.Env)
	}
}

func TestCredentialWithIllegalEnvNameIsRejected(t *testing.T) {
	_, err := alexsdk.NewTool("acme/bad-cred", "0.1.0").
		Description("atool with an illegal credential env name").
		Binary("bin/x").
		Credential(alexsdk.CredentialDecl{Env: "9-not-valid", Required: true}).
		Build()
	if err == nil {
		t.Fatal("expected build to reject illegal credential env var name")
	}
}

func TestCodeLessToolRoundTrip(t *testing.T) {
	dir := t.TempDir()
	out := filepath.Join(dir, "delegate-0.1.0.atool")

	schema := json.RawMessage(`{
		"type": "object",
		"required": ["objective"],
		"properties": {
			"objective": {"type": "string"},
			"acceptance": {"type": "array", "items": {"type": "string"}}
		}
	}`)

	tool := alexsdk.NewTool("acme/delegate", "0.1.0").
		Description("code-less delegation tool").
		NativeHandler("emit_trigger").
		InputSchema(schema)

	if _, err := tool.Pack(out); err != nil {
		t.Fatalf("Pack: %v", err)
	}

	got, err := alexsdk.Verify(out)
	if err != nil {
		t.Fatalf("Verify: %v", err)
	}
	if got.Kind != alexsdk.KindAtool {
		t.Fatalf("expected kind=atool, got %q", got.Kind)
	}
	if strings.Contains(string(got.Config), "\"binary\"") {
		t.Fatalf("code-less tool must omit binary, got config %s", got.Config)
	}
	ac, err := got.AtoolConfig()
	if err != nil {
		t.Fatalf("AtoolConfig: %v", err)
	}
	if ac.NativeHandler != "emit_trigger" {
		t.Fatalf("expected native_handler=emit_trigger, got %q", ac.NativeHandler)
	}
	if len(ac.InputSchema) == 0 {
		t.Fatal("expected input_schema to be present")
	}
	if ac.Binary != "" {
		t.Fatalf("expected empty binary, got %q", ac.Binary)
	}
}

func TestValidationRejectsCodeLessToolWithoutInputSchema(t *testing.T) {
	raw := map[string]interface{}{
		"schema_version": "2",
		"name":           "acme/bad-native",
		"version":        "0.1.0",
		"kind":           "atool",
		"description":    "code-less tool missing its input_schema",
		"config":         map[string]interface{}{"kind": "atool", "native_handler": "emit_trigger"},
	}
	b, _ := json.Marshal(raw)
	var m alexsdk.Manifest
	_ = json.Unmarshal(b, &m)
	if err := alexsdk.AssertValid(&m); err == nil {
		t.Fatal("expected validation error for code-less tool without input_schema")
	}
}

func TestValidationRejectsAtoolWithNeitherBinaryNorHandler(t *testing.T) {
	raw := map[string]interface{}{
		"schema_version": "2",
		"name":           "acme/empty-tool",
		"version":        "0.1.0",
		"kind":           "atool",
		"description":    "atool with neither binary nor native_handler",
		"config":         map[string]interface{}{"kind": "atool"},
	}
	b, _ := json.Marshal(raw)
	var m alexsdk.Manifest
	_ = json.Unmarshal(b, &m)
	if err := alexsdk.AssertValid(&m); err == nil {
		t.Fatal("expected validation error for atool with neither binary nor native_handler")
	}
}

func TestToolTransportHTTPRetaxesToMcp(t *testing.T) {
	tool := alexsdk.NewTool("acme/mcptool", "0.1.0").
		Description("an mcp daemon").
		Binary("bin/mcptool").
		Port(7800).
		Transport("http")

	m, err := tool.Build()
	if err != nil {
		t.Fatalf("Build: %v", err)
	}
	if m.Kind != alexsdk.KindMcp {
		t.Fatalf("expected kind=mcp, got %q", m.Kind)
	}
	mc, err := m.McpConfig()
	if err != nil {
		t.Fatalf("McpConfig: %v", err)
	}
	if mc.Transport != "http" {
		t.Fatalf("expected transport=http, got %q", mc.Transport)
	}
}

func TestToolTransportGrpcStaysAtool(t *testing.T) {
	m, err := alexsdk.NewTool("acme/g", "0.1.0").
		Description("grpc tool").
		Binary("bin/g").
		Transport("grpc").
		Build()
	if err != nil {
		t.Fatalf("Build: %v", err)
	}
	if m.Kind != alexsdk.KindAtool {
		t.Fatalf("expected kind=atool, got %q", m.Kind)
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
	if comp.Kind != "aagent" {
		t.Fatalf("expected component kind=aagent, got %q", comp.Kind)
	}
	if _, err := alexsdk.Verify(out); err != nil {
		t.Fatalf("Verify parent: %v", err)
	}
}

func TestAgentExtendsAndLockfile(t *testing.T) {
	dir := t.TempDir()

	agent := alexsdk.NewAgent("acme/child", "0.1.0").
		Description("child agent extending a base").
		SystemPrompt("You extend a base agent.").
		PromptMode("append").
		Extend(alexsdk.PackageDep{Name: "acme/base-agent", Version: "1.0.0"}).
		Lock(alexsdk.LockEntry{Name: "web-search", InterfaceMajor: 2})

	out := filepath.Join(dir, "child-0.1.0.aagent")
	m, err := agent.Pack(out)
	if err != nil {
		t.Fatalf("Pack: %v", err)
	}
	if len(m.Extends) != 1 || m.Extends[0].Name != "acme/base-agent" {
		t.Fatalf("unexpected extends: %+v", m.Extends)
	}
	if len(m.Lockfile) != 1 || m.Lockfile[0].InterfaceMajor != 2 {
		t.Fatalf("unexpected lockfile: %+v", m.Lockfile)
	}
	ac, err := m.AagentConfig()
	if err != nil {
		t.Fatalf("AagentConfig: %v", err)
	}
	if ac.PromptMode != "append" {
		t.Fatalf("expected prompt_mode=append, got %q", ac.PromptMode)
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
	raw := map[string]interface{}{
		"schema_version": "2",
		"name":           "acme/bad-tool",
		"version":        "0.1.0",
		"kind":           "atool",
		"description":    "tool with components",
		"config":         map[string]interface{}{"kind": "atool", "binary": "bin/x"},
		"components":     []interface{}{map[string]interface{}{"ref": "acme/foo@1.0.0"}},
	}
	b, _ := json.Marshal(raw)
	var m alexsdk.Manifest
	_ = json.Unmarshal(b, &m)
	if err := alexsdk.AssertValid(&m); err == nil {
		t.Fatal("expected validation error for components on tool")
	}
}

func TestValidationRejectsExtendsOnAtool(t *testing.T) {
	raw := map[string]interface{}{
		"schema_version": "2",
		"name":           "acme/bad-tool",
		"version":        "0.1.0",
		"kind":           "atool",
		"description":    "atool that wrongly carries extends",
		"config":         map[string]interface{}{"kind": "atool", "binary": "bin/x"},
		"extends":        []interface{}{map[string]interface{}{"name": "acme/base", "version": "1.0.0"}},
	}
	b, _ := json.Marshal(raw)
	var m alexsdk.Manifest
	_ = json.Unmarshal(b, &m)
	if err := alexsdk.AssertValid(&m); err == nil {
		t.Fatal("expected validation error for extends on atool")
	}
}

func TestValidationAcceptsRefToToolInAgentComponents(t *testing.T) {
	raw := map[string]interface{}{
		"schema_version": "2",
		"name":           "acme/agent-with-tool-ref",
		"version":        "0.1.0",
		"kind":           "aagent",
		"description":    "agent that refs a tool",
		"config":         map[string]interface{}{"kind": "aagent", "system_prompt": "hi"},
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

func TestSkillEmitsAagentWithModel(t *testing.T) {
	dir := t.TempDir()
	out := filepath.Join(dir, "skill-0.1.0.aagent")

	skill := alexsdk.NewSkill("acme/myskill", "0.1.0").
		Description("a prompt-only skill").
		SystemPrompt("You are specialized.").
		Model("claude-haiku")

	m, err := skill.Pack(out)
	if err != nil {
		t.Fatalf("Pack: %v", err)
	}
	if m.Kind != alexsdk.KindAagent {
		t.Fatalf("expected kind=aagent, got %q", m.Kind)
	}
	sc, err := m.AagentConfig()
	if err != nil {
		t.Fatalf("AagentConfig: %v", err)
	}
	if sc.Model != "claude-haiku" {
		t.Fatalf("expected model=claude-haiku, got %q", sc.Model)
	}
}

func TestInvalidManifestMissingDescription(t *testing.T) {
	a := alexsdk.NewAgent("acme/no-desc", "0.1.0").
		SystemPrompt("hello")
	if _, err := a.Build(); err == nil {
		t.Fatal("expected validation error for missing description")
	}
}

// --- Migration tests ---

func TestMigrateV1ToolBecomesMcpByDefault(t *testing.T) {
	v1 := map[string]interface{}{
		"schema_version": "1",
		"name":           "acme/mytool",
		"version":        "0.1.0",
		"kind":           "tool",
		"description":    "http tool",
		"config":         map[string]interface{}{"kind": "tool", "binary": "bin/x", "transport": "http"},
	}
	m, _, errs := alexsdk.MigrateManifest(v1)
	if len(errs) > 0 {
		t.Fatalf("unexpected errors: %v", errs)
	}
	if m["kind"] != "mcp" {
		t.Fatalf("expected kind=mcp, got %v", m["kind"])
	}
	cfg := m["config"].(map[string]interface{})
	if cfg["kind"] != "mcp" {
		t.Fatalf("expected config.kind=mcp, got %v", cfg["kind"])
	}
}

func TestMigrateV1GrpcToolBecomesAtool(t *testing.T) {
	v1 := map[string]interface{}{
		"schema_version": "1",
		"name":           "acme/mytool",
		"version":        "0.1.0",
		"kind":           "tool",
		"description":    "grpc tool",
		"config":         map[string]interface{}{"kind": "tool", "binary": "bin/x", "transport": "grpc"},
	}
	m, _, errs := alexsdk.MigrateManifest(v1)
	if len(errs) > 0 {
		t.Fatalf("unexpected errors: %v", errs)
	}
	if m["kind"] != "atool" {
		t.Fatalf("expected kind=atool, got %v", m["kind"])
	}
}

func TestMigrateV1AgentKeepsModel(t *testing.T) {
	v1 := map[string]interface{}{
		"schema_version": "1",
		"name":           "acme/myagent",
		"version":        "0.1.0",
		"kind":           "agent",
		"description":    "test agent",
		"config":         map[string]interface{}{"kind": "agent", "system_prompt": "hello", "model": "claude-opus-4-7"},
	}
	m, _, errs := alexsdk.MigrateManifest(v1)
	if len(errs) > 0 {
		t.Fatalf("unexpected errors: %v", errs)
	}
	if m["kind"] != "aagent" {
		t.Fatalf("expected kind=aagent, got %v", m["kind"])
	}
	cfg := m["config"].(map[string]interface{})
	if cfg["kind"] != "aagent" || cfg["model"] != "claude-opus-4-7" {
		t.Fatalf("unexpected config: %v", cfg)
	}
	if _, ok := cfg["llm"]; ok {
		t.Fatal("did not expect llm key")
	}
}

func TestMigrateV1SkillDropsTags(t *testing.T) {
	v1 := map[string]interface{}{
		"schema_version": "1",
		"name":           "acme/myskill",
		"version":        "0.2.0",
		"kind":           "skill",
		"description":    "a skill",
		"config": map[string]interface{}{
			"kind":          "skill",
			"system_prompt": "hi",
			"model_hint":    "claude-haiku",
			"tags":          []interface{}{"a", "b"},
		},
	}
	m, warnings, errs := alexsdk.MigrateManifest(v1)
	if len(errs) > 0 {
		t.Fatalf("unexpected errors: %v", errs)
	}
	if m["kind"] != "aagent" {
		t.Fatalf("expected kind=aagent, got %v", m["kind"])
	}
	cfg := m["config"].(map[string]interface{})
	if cfg["model"] != "claude-haiku" {
		t.Fatalf("expected model=claude-haiku, got %v", cfg["model"])
	}
	if _, ok := cfg["tags"]; ok {
		t.Fatal("expected tags to be dropped")
	}
	found := false
	for _, w := range warnings {
		if w == "config.tags removed (EE aagent has no tags field)" {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected tags-removed warning, got %v", warnings)
	}
}

func TestMigrateV1BundleToAagent(t *testing.T) {
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
	result, warnings, errs := alexsdk.MigrateManifest(v1)
	if len(errs) > 0 {
		t.Fatalf("unexpected errors: %v", errs)
	}
	if result["kind"] != "aagent" {
		t.Fatalf("expected kind=aagent, got %v", result["kind"])
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
	_, _, errs := alexsdk.MigrateManifest(v1)
	if len(errs) == 0 {
		t.Fatal("expected error for llm-runtime kind")
	}
}
