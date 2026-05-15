package alexsdk

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// PackOptions carries advanced options for builder pack invocations.
type PackOptions struct {
	SrcDir string
}

// PackOpt mutates PackOptions.
type PackOpt func(*PackOptions)

// WithSrcDir packs from an existing source dir rather than materialising a
// temp dir of staged files.
func WithSrcDir(dir string) PackOpt {
	return func(o *PackOptions) { o.SrcDir = dir }
}

type stagedFile struct {
	archivePath string
	srcAbs      string
}

type base struct {
	manifest Manifest
	staged   []stagedFile
}

func newBase(name, version string, kind Kind, config any) base {
	raw, _ := json.Marshal(config) // typed config; cannot fail
	return base{
		manifest: Manifest{
			SchemaVersion: "2",
			Name:          name,
			Version:       version,
			Kind:          kind,
			Description:   "",
			Config:        raw,
		},
	}
}

func (b *base) setConfig(c any) {
	raw, _ := json.Marshal(c)
	b.manifest.Config = raw
}

func (b *base) ensurePerms() *Permissions {
	if b.manifest.Permissions == nil {
		b.manifest.Permissions = &Permissions{}
	}
	return b.manifest.Permissions
}

func (b *base) build() (*Manifest, error) {
	// Deep copy via JSON round-trip so callers can keep mutating the builder.
	raw, err := json.Marshal(&b.manifest)
	if err != nil {
		return nil, fmt.Errorf("marshal manifest: %w", err)
	}
	var copy Manifest
	if err := json.Unmarshal(raw, &copy); err != nil {
		return nil, fmt.Errorf("clone manifest: %w", err)
	}
	if err := AssertValid(&copy); err != nil {
		return nil, err
	}
	return &copy, nil
}

func (b *base) packInternal(outPath string, opts []PackOpt) (*Manifest, error) {
	o := PackOptions{}
	for _, f := range opts {
		f(&o)
	}
	manifest, err := b.build()
	if err != nil {
		return nil, err
	}
	if o.SrcDir != "" {
		if err := writeManifestFile(o.SrcDir, manifest); err != nil {
			return nil, err
		}
		return Pack(o.SrcDir, outPath)
	}
	dir, err := os.MkdirTemp("", "alex-sdk-")
	if err != nil {
		return nil, fmt.Errorf("mkdtemp: %w", err)
	}
	if err := writeManifestFile(dir, manifest); err != nil {
		return nil, err
	}
	for _, s := range b.staged {
		dest := filepath.Join(dir, s.archivePath)
		if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
			return nil, fmt.Errorf("mkdir %s: %w", filepath.Dir(dest), err)
		}
		if err := copyFile(s.srcAbs, dest); err != nil {
			return nil, fmt.Errorf("stage %s: %w", s.archivePath, err)
		}
	}
	return Pack(dir, outPath)
}

func writeManifestFile(dir string, m *Manifest) error {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("mkdir %s: %w", dir, err)
	}
	data, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return fmt.Errorf("encode manifest: %w", err)
	}
	data = append(data, '\n')
	path := filepath.Join(dir, "atool.json")
	if err := os.WriteFile(path, data, 0o644); err != nil {
		return fmt.Errorf("write %s: %w", path, err)
	}
	return nil
}

func copyFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()
	_, err = io.Copy(out, in)
	return err
}

// Tool is a fluent builder for kind=tool packages.
type Tool struct {
	base
	cfg ToolConfig
}

// NewTool constructs a Tool builder with the given name and version.
func NewTool(name, version string) *Tool {
	t := &Tool{cfg: ToolConfig{Kind: "tool"}}
	t.base = newBase(name, version, KindTool, t.cfg)
	return t
}

func (t *Tool) sync() *Tool { t.setConfig(t.cfg); return t }

// Description sets the human-readable description.
func (t *Tool) Description(d string) *Tool { t.manifest.Description = d; return t }

// Author sets the author field.
func (t *Tool) Author(a string) *Tool { t.manifest.Author = a; return t }

// License sets the license field.
func (t *Tool) License(l string) *Tool { t.manifest.License = l; return t }

// RequiresAlexandria sets the minimum Alexandria version.
func (t *Tool) RequiresAlexandria(v string) *Tool { t.manifest.RequiresAlexandria = v; return t }

// Dependency appends a dependency to the manifest.
func (t *Tool) Dependency(d Dependency) *Tool {
	t.manifest.Dependencies = append(t.manifest.Dependencies, d)
	return t
}

// Dependencies replaces the dependencies slice.
func (t *Tool) Dependencies(ds []Dependency) *Tool { t.manifest.Dependencies = ds; return t }

// File appends a single file entry.
func (t *Tool) File(f FileEntry) *Tool { t.manifest.Files = append(t.manifest.Files, f); return t }

// Files replaces the files slice.
func (t *Tool) Files(fs []FileEntry) *Tool { t.manifest.Files = fs; return t }

// StageFile stages a file from disk to be copied into the temp src dir at
// pack time, and appends a matching files[] entry.
func (t *Tool) StageFile(srcPath string, entry FileEntry) *Tool {
	abs, _ := filepath.Abs(srcPath)
	t.staged = append(t.staged, stagedFile{archivePath: entry.ArchivePath, srcAbs: abs})
	return t.File(entry)
}

// ProvidesTools sets the permissions.provides_tools list.
func (t *Tool) ProvidesTools(s []string) *Tool { t.ensurePerms().ProvidesTools = s; return t }

// NeedsTools sets the permissions.needs_tools list.
func (t *Tool) NeedsTools(s []string) *Tool { t.ensurePerms().NeedsTools = s; return t }

// SuggestedRole sets the permissions.suggested_role hint.
func (t *Tool) SuggestedRole(r string) *Tool { t.ensurePerms().SuggestedRole = r; return t }

// Binary sets config.binary.
func (t *Tool) Binary(p string) *Tool { t.cfg.Binary = p; return t.sync() }

// Port sets config.default_port.
func (t *Tool) Port(p int) *Tool { v := p; t.cfg.DefaultPort = &v; return t.sync() }

// Transport sets config.transport (http|sse).
func (t *Tool) Transport(s string) *Tool { t.cfg.Transport = s; return t.sync() }

// Args sets config.args.
func (t *Tool) Args(a []string) *Tool { t.cfg.Args = a; return t.sync() }

// K8sImage sets config.k8s_image.
func (t *Tool) K8sImage(img string) *Tool { t.cfg.K8sImage = img; return t.sync() }

// K8sCapabilities sets config.k8s_capabilities.
func (t *Tool) K8sCapabilities(c []string) *Tool { t.cfg.K8sCapabilities = c; return t.sync() }

// K8sPort sets config.k8s_port.
func (t *Tool) K8sPort(p int) *Tool { v := p; t.cfg.K8sPort = &v; return t.sync() }

// K8sTransport sets config.k8s_transport (grpc|http|sse).
func (t *Tool) K8sTransport(s string) *Tool { t.cfg.K8sTransport = s; return t.sync() }

// K8sResources sets config.k8s_resources.
func (t *Tool) K8sResources(r K8sResources) *Tool { t.cfg.K8sResources = &r; return t.sync() }

// K8sMinWarm sets config.k8s_min_warm.
func (t *Tool) K8sMinWarm(n int) *Tool { v := n; t.cfg.K8sMinWarm = &v; return t.sync() }

// K8sIdleTimeout sets config.k8s_idle_timeout_seconds.
func (t *Tool) K8sIdleTimeout(seconds int) *Tool {
	v := seconds
	t.cfg.K8sIdleTimeoutSeconds = &v
	return t.sync()
}

// Build returns the validated manifest.
func (t *Tool) Build() (*Manifest, error) { t.sync(); return t.base.build() }

// Pack writes a .atool to outPath. See PackOpt for options.
func (t *Tool) Pack(outPath string, opts ...PackOpt) (*Manifest, error) {
	t.sync()
	return t.base.packInternal(outPath, opts)
}

// Agent is a fluent builder for kind=agent packages.
type Agent struct {
	base
	cfg AgentConfig
}

// NewAgent constructs an Agent builder.
func NewAgent(name, version string) *Agent {
	a := &Agent{cfg: AgentConfig{Kind: "agent"}}
	a.base = newBase(name, version, KindAgent, a.cfg)
	return a
}

func (a *Agent) sync() *Agent { a.setConfig(a.cfg); return a }

// Description sets the human-readable description.
func (a *Agent) Description(d string) *Agent { a.manifest.Description = d; return a }

// Author sets the author field.
func (a *Agent) Author(s string) *Agent { a.manifest.Author = s; return a }

// License sets the license field.
func (a *Agent) License(l string) *Agent { a.manifest.License = l; return a }

// RequiresAlexandria sets the minimum Alexandria version.
func (a *Agent) RequiresAlexandria(v string) *Agent { a.manifest.RequiresAlexandria = v; return a }

// Dependency appends a dependency.
func (a *Agent) Dependency(d Dependency) *Agent {
	a.manifest.Dependencies = append(a.manifest.Dependencies, d)
	return a
}

// Dependencies replaces the dependencies slice.
func (a *Agent) Dependencies(ds []Dependency) *Agent { a.manifest.Dependencies = ds; return a }

// File appends a single file entry.
func (a *Agent) File(f FileEntry) *Agent { a.manifest.Files = append(a.manifest.Files, f); return a }

// Files replaces the files slice.
func (a *Agent) Files(fs []FileEntry) *Agent { a.manifest.Files = fs; return a }

// StageFile stages a file from disk and appends a matching files[] entry.
func (a *Agent) StageFile(srcPath string, entry FileEntry) *Agent {
	abs, _ := filepath.Abs(srcPath)
	a.staged = append(a.staged, stagedFile{archivePath: entry.ArchivePath, srcAbs: abs})
	return a.File(entry)
}

// ProvidesTools sets permissions.provides_tools.
func (a *Agent) ProvidesTools(s []string) *Agent { a.ensurePerms().ProvidesTools = s; return a }

// NeedsTools sets permissions.needs_tools.
func (a *Agent) NeedsTools(s []string) *Agent { a.ensurePerms().NeedsTools = s; return a }

// SuggestedRole sets permissions.suggested_role.
func (a *Agent) SuggestedRole(r string) *Agent { a.ensurePerms().SuggestedRole = r; return a }

// SystemPrompt sets config.system_prompt.
func (a *Agent) SystemPrompt(s string) *Agent { a.cfg.SystemPrompt = s; return a.sync() }

// SystemPromptFromFile reads the system prompt from disk.
func (a *Agent) SystemPromptFromFile(p string) *Agent {
	data, err := os.ReadFile(p)
	if err != nil {
		a.cfg.SystemPrompt = ""
		return a.sync()
	}
	a.cfg.SystemPrompt = string(data)
	return a.sync()
}

// AllowedTools sets config.allowed_tools.
func (a *Agent) AllowedTools(t []string) *Agent { a.cfg.AllowedTools = t; return a.sync() }

// LLM sets config.llm (replaces v1 Model). Freeform preferred LLM id.
func (a *Agent) LLM(m string) *Agent { a.cfg.LLM = m; return a.sync() }

// HistoryLimit sets config.history_limit.
func (a *Agent) HistoryLimit(n int) *Agent { v := n; a.cfg.HistoryLimit = &v; return a.sync() }

// Componentable is any builder that can be embedded as an inline component
// in an Agent's components[]. Agent and Skill both implement it.
type Componentable interface {
	Build() (*Manifest, error)
}

// Component appends an inline sub-agent or sub-skill to components[].
// name is the local label; id is the canonical ns/name@version. child may be
// either *Agent or *Skill (anything implementing Componentable).
func (a *Agent) Component(name, id string, child Componentable) *Agent {
	a.sync()
	m, err := child.Build()
	if err != nil {
		// Defer error to Build() of this agent by appending nothing;
		// schema validation will catch any missing required component.
		return a
	}
	raw, _ := json.Marshal(m.Config)
	item := ComponentItem{
		Name:   name,
		ID:     id,
		Kind:   string(m.Kind),
		Config: raw,
	}
	if len(m.Files) > 0 {
		item.Files = m.Files
	}
	if m.Permissions != nil {
		item.Permissions = m.Permissions
	}
	if len(m.Dependencies) > 0 {
		item.Dependencies = m.Dependencies
	}
	if len(m.Components) > 0 {
		item.Components = m.Components
	}
	a.manifest.Components = append(a.manifest.Components, item)
	return a
}

// Ref appends an external ref component (any kind: tool, skill, or agent).
func (a *Agent) Ref(nsNameAtVersion string) *Agent {
	a.sync()
	a.manifest.Components = append(a.manifest.Components, ComponentItem{Ref: nsNameAtVersion})
	return a
}

// Flatten sets install.flatten merge rules.
func (a *Agent) Flatten(f InstallFlatten) *Agent {
	a.sync()
	if a.manifest.Install == nil {
		a.manifest.Install = &InstallBlock{}
	}
	a.manifest.Install.Flatten = &f
	return a
}

// Build returns the validated manifest.
func (a *Agent) Build() (*Manifest, error) { a.sync(); return a.base.build() }

// Pack writes a .aagent to outPath.
func (a *Agent) Pack(outPath string, opts ...PackOpt) (*Manifest, error) {
	a.sync()
	return a.base.packInternal(outPath, opts)
}

// Skill is a fluent builder for kind=skill packages.
type Skill struct {
	base
	cfg SkillConfig
}

// NewSkill constructs a Skill builder.
func NewSkill(name, version string) *Skill {
	s := &Skill{cfg: SkillConfig{Kind: "skill"}}
	s.base = newBase(name, version, KindSkill, s.cfg)
	return s
}

func (s *Skill) sync() *Skill { s.setConfig(s.cfg); return s }

// Description sets the human-readable description.
func (s *Skill) Description(d string) *Skill { s.manifest.Description = d; return s }

// Author sets the author field.
func (s *Skill) Author(a string) *Skill { s.manifest.Author = a; return s }

// License sets the license field.
func (s *Skill) License(l string) *Skill { s.manifest.License = l; return s }

// RequiresAlexandria sets the minimum Alexandria version.
func (s *Skill) RequiresAlexandria(v string) *Skill { s.manifest.RequiresAlexandria = v; return s }

// Dependency appends a dependency.
func (s *Skill) Dependency(d Dependency) *Skill {
	s.manifest.Dependencies = append(s.manifest.Dependencies, d)
	return s
}

// Dependencies replaces the dependencies slice.
func (s *Skill) Dependencies(ds []Dependency) *Skill { s.manifest.Dependencies = ds; return s }

// File appends a single file entry.
func (s *Skill) File(f FileEntry) *Skill { s.manifest.Files = append(s.manifest.Files, f); return s }

// Files replaces the files slice.
func (s *Skill) Files(fs []FileEntry) *Skill { s.manifest.Files = fs; return s }

// StageFile stages a file from disk and appends a matching files[] entry.
func (s *Skill) StageFile(srcPath string, entry FileEntry) *Skill {
	abs, _ := filepath.Abs(srcPath)
	s.staged = append(s.staged, stagedFile{archivePath: entry.ArchivePath, srcAbs: abs})
	return s.File(entry)
}

// ProvidesTools sets permissions.provides_tools.
func (s *Skill) ProvidesTools(t []string) *Skill { s.ensurePerms().ProvidesTools = t; return s }

// NeedsTools sets permissions.needs_tools.
func (s *Skill) NeedsTools(t []string) *Skill { s.ensurePerms().NeedsTools = t; return s }

// SuggestedRole sets permissions.suggested_role.
func (s *Skill) SuggestedRole(r string) *Skill { s.ensurePerms().SuggestedRole = r; return s }

// SystemPrompt sets config.system_prompt.
func (s *Skill) SystemPrompt(p string) *Skill { s.cfg.SystemPrompt = p; return s.sync() }

// AllowedTools sets config.allowed_tools.
func (s *Skill) AllowedTools(t []string) *Skill { s.cfg.AllowedTools = t; return s.sync() }

// LLM sets config.llm (replaces v1 ModelHint). Freeform preferred LLM id.
func (s *Skill) LLM(m string) *Skill { s.cfg.LLM = m; return s.sync() }

// Tags sets config.tags.
func (s *Skill) Tags(t []string) *Skill { s.cfg.Tags = t; return s.sync() }

// Build returns the validated manifest.
func (s *Skill) Build() (*Manifest, error) { s.sync(); return s.base.build() }

// Pack writes a .atool to outPath.
func (s *Skill) Pack(outPath string, opts ...PackOpt) (*Manifest, error) {
	s.sync()
	return s.base.packInternal(outPath, opts)
}

func shortName(n string) string {
	if i := strings.LastIndex(n, "/"); i >= 0 {
		return n[i+1:]
	}
	return n
}
