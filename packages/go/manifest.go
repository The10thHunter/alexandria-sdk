// Package alexsdk authors and verifies Alexandria .atool / .aagent packages
// (EE-canonical schema v2).
//
// Kinds mirror ee/crates/alex-package/src/manifest.rs:
//
//	mcp    — binary tool daemon over the MCP protocol (JSON-RPC/SSE)
//	atool  — binary tool daemon over the native gRPC ToolService
//	aagent — orchestrator-managed agent. A "skill" is reusable prompt text that
//	         ships as an aagent whose content is its system_prompt — there is no
//	         standalone skill kind.
package alexsdk

import (
	"encoding/json"
	"fmt"
)

// Kind enumerates the package kinds understood by Alexandria.
type Kind string

// Known package kinds. Keep in sync with atool.schema.json.
const (
	KindMcp    Kind = "mcp"
	KindAtool  Kind = "atool"
	KindAagent Kind = "aagent"
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

// Dependency declares a required sibling package. Version is required.
type Dependency struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

// PackageDep is a base-package reference used by aagent Extends. Version is
// optional (EE PackageDep has a serde default).
type PackageDep struct {
	Name    string `json:"name"`
	Version string `json:"version,omitempty"`
}

// LockEntry is one resolved entry in an aagent's inheritance lockfile.
type LockEntry struct {
	Name           string `json:"name"`
	InterfaceMajor int    `json:"interface_major"`
	ContractHash   string `json:"contract_hash,omitempty"`
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

// McpConfig is the typed `config` block for kind=mcp (MCP JSON-RPC/SSE).
type McpConfig struct {
	Kind                  string        `json:"kind"`
	Binary                string        `json:"binary"`
	DefaultPort           *int          `json:"default_port,omitempty"`
	Transport             string        `json:"transport,omitempty"`
	Args                  []string      `json:"args,omitempty"`
	InterfaceMajor        *int          `json:"interface_major,omitempty"`
	K8sImage              string        `json:"k8s_image,omitempty"`
	K8sCapabilities       []string      `json:"k8s_capabilities,omitempty"`
	K8sPort               *int          `json:"k8s_port,omitempty"`
	K8sTransport          string        `json:"k8s_transport,omitempty"`
	K8sResources          *K8sResources `json:"k8s_resources,omitempty"`
	K8sMinWarm            *int          `json:"k8s_min_warm,omitempty"`
	K8sIdleTimeoutSeconds *int          `json:"k8s_idle_timeout_seconds,omitempty"`
}

// AtoolConfig is the typed `config` block for kind=atool (native gRPC).
type AtoolConfig struct {
	Kind                  string        `json:"kind"`
	Binary                string        `json:"binary"`
	DefaultPort           *int          `json:"default_port,omitempty"`
	Transport             string        `json:"transport,omitempty"`
	Args                  []string      `json:"args,omitempty"`
	InterfaceMajor        *int          `json:"interface_major,omitempty"`
	K8sImage              string        `json:"k8s_image,omitempty"`
	K8sCapabilities       []string      `json:"k8s_capabilities,omitempty"`
	K8sPort               *int          `json:"k8s_port,omitempty"`
	K8sTransport          string        `json:"k8s_transport,omitempty"`
	K8sResources          *K8sResources `json:"k8s_resources,omitempty"`
	K8sMinWarm            *int          `json:"k8s_min_warm,omitempty"`
	K8sIdleTimeoutSeconds *int          `json:"k8s_idle_timeout_seconds,omitempty"`
}

// AagentConfig is the typed `config` block for kind=aagent. A skill collapses
// into this shape with only SystemPrompt populated — there is no Tags field.
type AagentConfig struct {
	Kind         string   `json:"kind"`
	SystemPrompt string   `json:"system_prompt"`
	AllowedTools []string `json:"allowed_tools,omitempty"`
	// Model is the preferred model backend id (EE `model`). Replaces v1 Model/ModelHint.
	Model        string `json:"model,omitempty"`
	HistoryLimit *int   `json:"history_limit,omitempty"`
	// PromptMode composes this prompt with Extends bases: "append" | "replace".
	PromptMode string `json:"prompt_mode,omitempty"`
}

// InstallFlatten defines merge rules for components at install time.
type InstallFlatten struct {
	SystemPrompt string `json:"system_prompt,omitempty"`
	AllowedTools string `json:"allowed_tools,omitempty"`
	Model        string `json:"model,omitempty"`
	HistoryLimit string `json:"history_limit,omitempty"`
}

// InstallBlock contains install-time options for aagents with components.
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

// ComponentItem is a discriminated union: either an inline sub-agent or an
// external reference. If Ref is non-empty, it is an external ref. Otherwise
// Name/ID/Kind/Config describe an inline sub-agent (always kind=aagent).
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
	SchemaVersion      string       `json:"schema_version"`
	Name               string       `json:"name"`
	Version            string       `json:"version"`
	Kind               Kind         `json:"kind"`
	Description        string       `json:"description"`
	Author             string       `json:"author,omitempty"`
	License            string       `json:"license,omitempty"`
	RequiresAlexandria string       `json:"requires_alexandria,omitempty"`
	Dependencies       []Dependency `json:"dependencies,omitempty"`
	// Extends lists base packages this aagent extends. aagent-only.
	Extends []PackageDep `json:"extends,omitempty"`
	// Lockfile is the resolved inheritance lockfile (aagent-only).
	Lockfile    []LockEntry     `json:"lockfile,omitempty"`
	Files       []FileEntry     `json:"files,omitempty"`
	Permissions *Permissions    `json:"permissions,omitempty"`
	Config      json.RawMessage `json:"config"`
	// Components is only valid on kind=aagent.
	Components []ComponentItem `json:"components,omitempty"`
	// Install is only valid on aagents with non-empty components.
	Install   *InstallBlock   `json:"install,omitempty"`
	Signature *SignatureBlock `json:"signature,omitempty"`
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

// McpConfig decodes m.Config as an McpConfig. Returns an error if Kind != mcp.
func (m *Manifest) McpConfig() (*McpConfig, error) {
	if m.Kind != KindMcp {
		return nil, fmt.Errorf("manifest kind is %q, not mcp", m.Kind)
	}
	var c McpConfig
	if err := json.Unmarshal(m.Config, &c); err != nil {
		return nil, fmt.Errorf("decode mcp config: %w", err)
	}
	return &c, nil
}

// AtoolConfig decodes m.Config as an AtoolConfig. Returns an error if Kind != atool.
func (m *Manifest) AtoolConfig() (*AtoolConfig, error) {
	if m.Kind != KindAtool {
		return nil, fmt.Errorf("manifest kind is %q, not atool", m.Kind)
	}
	var c AtoolConfig
	if err := json.Unmarshal(m.Config, &c); err != nil {
		return nil, fmt.Errorf("decode atool config: %w", err)
	}
	return &c, nil
}

// AagentConfig decodes m.Config as an AagentConfig. Returns an error if Kind != aagent.
func (m *Manifest) AagentConfig() (*AagentConfig, error) {
	if m.Kind != KindAagent {
		return nil, fmt.Errorf("manifest kind is %q, not aagent", m.Kind)
	}
	var c AagentConfig
	if err := json.Unmarshal(m.Config, &c); err != nil {
		return nil, fmt.Errorf("decode aagent config: %w", err)
	}
	return &c, nil
}

// IntPtr returns a pointer to v. Convenience for setting optional integer fields.
func IntPtr(v int) *int { return &v }
