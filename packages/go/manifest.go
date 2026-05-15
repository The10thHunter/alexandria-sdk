// Package alexsdk authors and verifies Alexandria .atool / .aagent packages.
package alexsdk

import (
	"encoding/json"
	"fmt"
)

// Kind enumerates the package kinds understood by Alexandria.
type Kind string

// Known package kinds. Keep in sync with atool.schema.json.
const (
	KindTool  Kind = "tool"
	KindAgent Kind = "agent"
	KindSkill Kind = "skill"
)

// FileEntry is a declared file inside the package archive.
type FileEntry struct {
	ArchivePath string `json:"archive_path"`
	InstallPath string `json:"install_path"`
	Executable  bool   `json:"executable,omitempty"`
	SHA256      string `json:"sha256,omitempty"`
}

// Permissions controls what tools a package provides or needs at runtime.
type Permissions struct {
	ProvidesTools []string `json:"provides_tools,omitempty"`
	NeedsTools    []string `json:"needs_tools,omitempty"`
	SuggestedRole string   `json:"suggested_role,omitempty"`
}

// Dependency declares a required sibling package.
type Dependency struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

// K8sResourceLimits expresses cpu/memory under requests or limits.
type K8sResourceLimits struct {
	CPU    string `json:"cpu,omitempty"`
	Memory string `json:"memory,omitempty"`
}

// K8sResources holds requests/limits blocks for Kubernetes-launched tools.
type K8sResources struct {
	Requests *K8sResourceLimits `json:"requests,omitempty"`
	Limits   *K8sResourceLimits `json:"limits,omitempty"`
}

// ToolConfig is the typed `config` block for kind=tool.
type ToolConfig struct {
	Kind                  string        `json:"kind"`
	Binary                string        `json:"binary"`
	DefaultPort           *int          `json:"default_port,omitempty"`
	Transport             string        `json:"transport,omitempty"`
	Args                  []string      `json:"args,omitempty"`
	K8sImage              string        `json:"k8s_image,omitempty"`
	K8sCapabilities       []string      `json:"k8s_capabilities,omitempty"`
	K8sPort               *int          `json:"k8s_port,omitempty"`
	K8sTransport          string        `json:"k8s_transport,omitempty"`
	K8sResources          *K8sResources `json:"k8s_resources,omitempty"`
	K8sMinWarm            *int          `json:"k8s_min_warm,omitempty"`
	K8sIdleTimeoutSeconds *int          `json:"k8s_idle_timeout_seconds,omitempty"`
}

// AgentConfig is the typed `config` block for kind=agent.
type AgentConfig struct {
	Kind         string   `json:"kind"`
	SystemPrompt string   `json:"system_prompt"`
	AllowedTools []string `json:"allowed_tools,omitempty"`
	// LLM is a freeform preferred LLM id. Replaces v1 Model.
	LLM          string   `json:"llm,omitempty"`
	HistoryLimit *int     `json:"history_limit,omitempty"`
}

// SkillConfig is the typed `config` block for kind=skill.
type SkillConfig struct {
	Kind         string   `json:"kind"`
	SystemPrompt string   `json:"system_prompt"`
	AllowedTools []string `json:"allowed_tools,omitempty"`
	// LLM is a freeform preferred LLM id. Replaces v1 ModelHint.
	LLM          string   `json:"llm,omitempty"`
	Tags         []string `json:"tags,omitempty"`
}

// InstallFlatten defines merge rules for components at install time.
type InstallFlatten struct {
	SystemPrompt string `json:"system_prompt,omitempty"`
	AllowedTools string `json:"allowed_tools,omitempty"`
	LLM          string `json:"llm,omitempty"`
	HistoryLimit string `json:"history_limit,omitempty"`
}

// InstallBlock contains install-time options for agents with components.
type InstallBlock struct {
	Flatten *InstallFlatten `json:"flatten,omitempty"`
}

// SignatureBlock holds cryptographic signature metadata.
type SignatureBlock struct {
	Alg            string `json:"alg"`
	KeyFingerprint string `json:"key_fingerprint"`
	Value          string `json:"value"`
	Scope          string `json:"scope"`
}

// ComponentItem is a discriminated union: either an inline sub-component or
// an external reference.
// If Ref is non-empty, it is an external ref. Otherwise Name/ID/Kind/Config
// describe an inline sub-agent or sub-skill.
type ComponentItem struct {
	// Ref: external reference (ns/name@version). Non-empty means this is a ref.
	Ref string `json:"ref,omitempty"`

	// Inline fields (mutually exclusive with Ref).
	Name         string          `json:"name,omitempty"`
	ID           string          `json:"id,omitempty"`
	Kind         string          `json:"kind,omitempty"`
	Config       json.RawMessage `json:"config,omitempty"`
	Components   []ComponentItem `json:"components,omitempty"`
	Files        []FileEntry     `json:"files,omitempty"`
	Permissions  *Permissions    `json:"permissions,omitempty"`
	Dependencies []Dependency    `json:"dependencies,omitempty"`
}

// Manifest is the wire format for atool.json.
type Manifest struct {
	SchemaVersion      string          `json:"schema_version"`
	Name               string          `json:"name"`
	Version            string          `json:"version"`
	Kind               Kind            `json:"kind"`
	Description        string          `json:"description"`
	Author             string          `json:"author,omitempty"`
	License            string          `json:"license,omitempty"`
	RequiresAlexandria string          `json:"requires_alexandria,omitempty"`
	Dependencies       []Dependency    `json:"dependencies,omitempty"`
	Files              []FileEntry     `json:"files,omitempty"`
	Permissions        *Permissions    `json:"permissions,omitempty"`
	Config             json.RawMessage `json:"config"`
	// Components is only valid on kind=agent.
	Components []ComponentItem `json:"components,omitempty"`
	// Install is only valid on agents with non-empty components.
	Install   *InstallBlock   `json:"install,omitempty"`
	Signature *SignatureBlock  `json:"signature,omitempty"`
}

// MarshalConfig serialises a typed config struct into a json.RawMessage suitable
// for Manifest.Config.
func MarshalConfig(c any) (json.RawMessage, error) {
	b, err := json.Marshal(c)
	if err != nil {
		return nil, fmt.Errorf("marshal config: %w", err)
	}
	return b, nil
}

// ToolConfig decodes m.Config as a ToolConfig. Returns an error if Kind != tool.
func (m *Manifest) ToolConfig() (*ToolConfig, error) {
	if m.Kind != KindTool {
		return nil, fmt.Errorf("manifest kind is %q, not tool", m.Kind)
	}
	var c ToolConfig
	if err := json.Unmarshal(m.Config, &c); err != nil {
		return nil, fmt.Errorf("decode tool config: %w", err)
	}
	return &c, nil
}

// AgentConfig decodes m.Config as an AgentConfig. Returns an error if Kind != agent.
func (m *Manifest) AgentConfig() (*AgentConfig, error) {
	if m.Kind != KindAgent {
		return nil, fmt.Errorf("manifest kind is %q, not agent", m.Kind)
	}
	var c AgentConfig
	if err := json.Unmarshal(m.Config, &c); err != nil {
		return nil, fmt.Errorf("decode agent config: %w", err)
	}
	return &c, nil
}

// SkillConfig decodes m.Config as a SkillConfig. Returns an error if Kind != skill.
func (m *Manifest) SkillConfig() (*SkillConfig, error) {
	if m.Kind != KindSkill {
		return nil, fmt.Errorf("manifest kind is %q, not skill", m.Kind)
	}
	var c SkillConfig
	if err := json.Unmarshal(m.Config, &c); err != nil {
		return nil, fmt.Errorf("decode skill config: %w", err)
	}
	return &c, nil
}

// IntPtr returns a pointer to v. Convenience for setting optional integer fields.
func IntPtr(v int) *int { return &v }
