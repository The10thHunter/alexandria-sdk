//! Fluent builders mirroring the TypeScript SDK (EE-canonical schema v2).
//!
//! Each builder owns a [`Manifest`] plus a `Vec<(archive_path, src_path)>` of
//! staged source files. `.pack(out)` materialises a tempdir, copies staged
//! files into it, writes `atool.json`, and delegates to [`crate::pack::pack`].
//!
//! All chainable setters take `mut self -> Self` so callers can write
//! `Agent::new(...).description(...).system_prompt(...).pack(out)` without
//! intermediate bindings.

use std::path::{Path, PathBuf};

use crate::manifest::{
    AagentConfig, AtoolConfig, ComponentItem, Dependency, FileEntry, InlineComponent,
    InlineComponentKind, InlineConfig, InstallBlock, InstallFlatten, K8sHints, K8sResources, Kind,
    LockEntry, Manifest, McpConfig, McpTransport, PackageConfig, PackageDep, Permissions,
    PromptMode, RefComponent, WireTransport,
};
use crate::pack::{self, write_manifest};
use crate::schema;
use crate::Result;

/// Common state shared by every builder.
struct Inner {
    manifest: Manifest,
    /// `(archive_path, src_abs_path)` pairs to copy into the staging tempdir.
    staged: Vec<(String, PathBuf)>,
}

impl Inner {
    fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        kind: Kind,
        config: PackageConfig,
    ) -> Self {
        Self {
            manifest: Manifest {
                schema_version: "2".to_string(),
                name: name.into(),
                version: version.into(),
                kind,
                description: String::new(),
                author: None,
                license: None,
                requires_alexandria: None,
                dependencies: None,
                extends: None,
                lockfile: None,
                files: None,
                permissions: None,
                config,
                components: None,
                install: None,
                signature: None,
            },
            staged: Vec::new(),
        }
    }

    fn ensure_perms(&mut self) -> &mut Permissions {
        self.manifest
            .permissions
            .get_or_insert_with(Permissions::default)
    }

    fn build(&self) -> Result<Manifest> {
        let value = serde_json::to_value(&self.manifest)?;
        schema::assert_valid(&value)?;
        Ok(self.manifest.clone())
    }

    /// Materialise the staged dir, write `atool.json`, and pack to `out_path`.
    fn pack_to(&self, out_path: &Path) -> Result<Manifest> {
        let manifest = self.build()?;
        let dir = tempfile::Builder::new().prefix("alex-sdk-").tempdir()?;
        write_manifest(dir.path(), &manifest)?;
        for (archive_path, src) in &self.staged {
            let dest = dir.path().join(archive_path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(src, &dest)?;
        }
        pack::pack(dir.path(), out_path)
    }
}

/// Macro: generate the common `description / author / license / ...` chainable
/// setters on every public builder.
macro_rules! common_builder_methods {
    () => {
        /// Set the package's free-text description (required by the schema).
        pub fn description(mut self, d: impl Into<String>) -> Self {
            self.inner.manifest.description = d.into();
            self
        }

        pub fn author(mut self, a: impl Into<String>) -> Self {
            self.inner.manifest.author = Some(a.into());
            self
        }

        pub fn license(mut self, l: impl Into<String>) -> Self {
            self.inner.manifest.license = Some(l.into());
            self
        }

        pub fn requires_alexandria(mut self, v: impl Into<String>) -> Self {
            self.inner.manifest.requires_alexandria = Some(v.into());
            self
        }

        pub fn dependency(mut self, d: Dependency) -> Self {
            self.inner
                .manifest
                .dependencies
                .get_or_insert_with(Vec::new)
                .push(d);
            self
        }

        pub fn dependencies(mut self, ds: Vec<Dependency>) -> Self {
            self.inner.manifest.dependencies = Some(ds);
            self
        }

        pub fn file(mut self, f: FileEntry) -> Self {
            self.inner
                .manifest
                .files
                .get_or_insert_with(Vec::new)
                .push(f);
            self
        }

        pub fn files(mut self, fs: Vec<FileEntry>) -> Self {
            self.inner.manifest.files = Some(fs);
            self
        }

        pub fn provides_tools(mut self, t: Vec<String>) -> Self {
            self.inner.ensure_perms().provides_tools = Some(t);
            self
        }

        pub fn needs_tools(mut self, t: Vec<String>) -> Self {
            self.inner.ensure_perms().needs_tools = Some(t);
            self
        }

        pub fn suggested_role(mut self, r: impl Into<String>) -> Self {
            self.inner.ensure_perms().suggested_role = Some(r.into());
            self
        }

        /// Stage a file from disk so [`Self::pack`] can include it without a
        /// pre-laid-out source dir. Automatically appends a matching
        /// `files[]` entry.
        pub fn stage_file(
            mut self,
            src_path: impl AsRef<Path>,
            archive_path: impl Into<String>,
            install_path: impl Into<String>,
            executable: bool,
        ) -> Self {
            let archive_path = archive_path.into();
            let install_path = install_path.into();
            let abs = std::fs::canonicalize(src_path.as_ref())
                .unwrap_or_else(|_| src_path.as_ref().to_path_buf());
            self.inner.staged.push((archive_path.clone(), abs));
            self.inner
                .manifest
                .files
                .get_or_insert_with(Vec::new)
                .push(FileEntry {
                    archive_path,
                    install_path,
                    executable: Some(executable),
                    sha256: None,
                });
            self
        }

        /// Validate and return the manifest.
        pub fn build(&self) -> Result<Manifest> {
            self.inner.build()
        }

        /// Pack to `out_path`. Staged files are copied into a tempdir.
        pub fn pack(&self, out_path: impl AsRef<Path>) -> Result<Manifest> {
            self.inner.pack_to(out_path.as_ref())
        }
    };
}

// ---------------------------------------------------------------------------
// Tool (kind = atool by default; kind = mcp when transport is http/sse)
// ---------------------------------------------------------------------------

/// Neutral field bag so the [`Tool`] builder can switch between mcp/atool
/// config variants as the transport changes.
#[derive(Default)]
struct ToolFields {
    binary: String,
    default_port: Option<u16>,
    transport: Option<WireTransport>,
    args: Option<Vec<String>>,
    interface_major: Option<u32>,
    k8s: K8sHints,
}

/// Builder for binary-tool packages.
///
/// Emits `kind = atool` (native gRPC `ToolService`) by default; calling
/// `.transport(WireTransport::Http | Sse)` re-taxes the package to `kind = mcp`
/// (MCP JSON-RPC/SSE). `.transport(WireTransport::Grpc)` keeps it an atool.
pub struct Tool {
    inner: Inner,
    fields: ToolFields,
}

impl Tool {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        let fields = ToolFields::default();
        let (kind, config) = Self::materialise(&fields);
        Self {
            inner: Inner::new(name, version, kind, config),
            fields,
        }
    }

    /// Build the (kind, config) pair from the neutral field bag. Transport
    /// grpc/none => atool; http/sse => mcp.
    fn materialise(f: &ToolFields) -> (Kind, PackageConfig) {
        match f.transport {
            Some(WireTransport::Http) | Some(WireTransport::Sse) => {
                let t = match f.transport {
                    Some(WireTransport::Http) => Some(McpTransport::Http),
                    Some(WireTransport::Sse) => Some(McpTransport::Sse),
                    _ => None,
                };
                let cfg = McpConfig {
                    binary: f.binary.clone(),
                    default_port: f.default_port,
                    transport: t,
                    args: f.args.clone(),
                    interface_major: f.interface_major,
                    k8s: f.k8s.clone(),
                };
                (Kind::Mcp, PackageConfig::Mcp(cfg))
            }
            _ => {
                let cfg = AtoolConfig {
                    binary: f.binary.clone(),
                    default_port: f.default_port,
                    transport: f.transport,
                    args: f.args.clone(),
                    interface_major: f.interface_major,
                    k8s: f.k8s.clone(),
                };
                (Kind::Atool, PackageConfig::Atool(cfg))
            }
        }
    }

    /// Re-derive kind + config into the manifest after a field change.
    fn rebuild(&mut self) {
        let (kind, config) = Self::materialise(&self.fields);
        self.inner.manifest.kind = kind;
        self.inner.manifest.config = config;
    }

    pub fn binary(mut self, p: impl Into<String>) -> Self {
        self.fields.binary = p.into();
        self.rebuild();
        self
    }
    pub fn port(mut self, p: u16) -> Self {
        self.fields.default_port = Some(p);
        self.rebuild();
        self
    }
    /// Pick the wire protocol — and thereby the package kind:
    /// `Grpc` => kind atool; `Http`/`Sse` => kind mcp.
    pub fn transport(mut self, t: WireTransport) -> Self {
        self.fields.transport = Some(t);
        self.rebuild();
        self
    }
    pub fn args(mut self, a: Vec<String>) -> Self {
        self.fields.args = Some(a);
        self.rebuild();
        self
    }
    /// Contract/ABI major this tool exposes over its wire protocol (EE default 1).
    pub fn interface_major(mut self, n: u32) -> Self {
        self.fields.interface_major = Some(n);
        self.rebuild();
        self
    }
    pub fn k8s_image(mut self, img: impl Into<String>) -> Self {
        self.fields.k8s.k8s_image = Some(img.into());
        self.rebuild();
        self
    }
    pub fn k8s_capabilities(mut self, c: Vec<String>) -> Self {
        self.fields.k8s.k8s_capabilities = Some(c);
        self.rebuild();
        self
    }
    pub fn k8s_port(mut self, p: u16) -> Self {
        self.fields.k8s.k8s_port = Some(p);
        self.rebuild();
        self
    }
    pub fn k8s_transport(mut self, t: crate::manifest::ToolK8sTransport) -> Self {
        self.fields.k8s.k8s_transport = Some(t);
        self.rebuild();
        self
    }
    pub fn k8s_resources(mut self, r: K8sResources) -> Self {
        self.fields.k8s.k8s_resources = Some(r);
        self.rebuild();
        self
    }
    pub fn k8s_min_warm(mut self, n: u32) -> Self {
        self.fields.k8s.k8s_min_warm = Some(n);
        self.rebuild();
        self
    }
    pub fn k8s_idle_timeout(mut self, seconds: u32) -> Self {
        self.fields.k8s.k8s_idle_timeout_seconds = Some(seconds);
        self.rebuild();
        self
    }

    common_builder_methods!();
}

// ---------------------------------------------------------------------------
// Agent (kind = aagent)
// ---------------------------------------------------------------------------

/// Builder for `kind: aagent` packages.
pub struct Agent {
    inner: Inner,
}

impl Agent {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        let config = PackageConfig::Aagent(AagentConfig {
            system_prompt: String::new(),
            allowed_tools: None,
            model: None,
            history_limit: None,
            prompt_mode: None,
        });
        Self {
            inner: Inner::new(name, version, Kind::Aagent, config),
        }
    }

    fn cfg(&mut self) -> &mut AagentConfig {
        match &mut self.inner.manifest.config {
            PackageConfig::Aagent(c) => c,
            _ => unreachable!("Agent builder always holds an AagentConfig"),
        }
    }

    pub fn system_prompt(mut self, s: impl Into<String>) -> Self {
        self.cfg().system_prompt = s.into();
        self
    }

    pub fn system_prompt_from_file(mut self, path: impl AsRef<Path>) -> Result<Self> {
        let s = std::fs::read_to_string(path.as_ref())?;
        self.cfg().system_prompt = s;
        Ok(self)
    }

    pub fn allowed_tools(mut self, t: Vec<String>) -> Self {
        self.cfg().allowed_tools = Some(t);
        self
    }

    /// Preferred model backend id (EE `config.model`). Replaces v1 `.llm()`.
    pub fn model(mut self, m: impl Into<String>) -> Self {
        self.cfg().model = Some(m.into());
        self
    }

    pub fn history_limit(mut self, n: u32) -> Self {
        self.cfg().history_limit = Some(n);
        self
    }

    /// Prompt composition mode against `extends` bases: append (default) | replace.
    pub fn prompt_mode(mut self, m: PromptMode) -> Self {
        self.cfg().prompt_mode = Some(m);
        self
    }

    /// Append a base package this aagent extends. aagent-only in EE.
    pub fn extend(mut self, base: PackageDep) -> Self {
        self.inner
            .manifest
            .extends
            .get_or_insert_with(Vec::new)
            .push(base);
        self
    }

    pub fn extends_packages(mut self, bases: Vec<PackageDep>) -> Self {
        self.inner.manifest.extends = Some(bases);
        self
    }

    /// Append a resolved inheritance lockfile entry (aagent-only).
    pub fn lock(mut self, entry: LockEntry) -> Self {
        self.inner
            .manifest
            .lockfile
            .get_or_insert_with(Vec::new)
            .push(entry);
        self
    }

    pub fn lockfile(mut self, entries: Vec<LockEntry>) -> Self {
        self.inner.manifest.lockfile = Some(entries);
        self
    }

    /// Append an inline sub-agent to components[].
    /// `name` is the local label; `id` is the canonical ns/name@version.
    pub fn component(
        mut self,
        name: impl Into<String>,
        id: impl Into<String>,
        child: Agent,
    ) -> Result<Self> {
        let child_manifest = child.build()?;
        self.push_inline(name.into(), id.into(), child_manifest);
        Ok(self)
    }

    /// Append an inline sub-skill to components[] (also emits kind=aagent).
    pub fn component_skill(
        mut self,
        name: impl Into<String>,
        id: impl Into<String>,
        child: Skill,
    ) -> Result<Self> {
        let child_manifest = child.build()?;
        self.push_inline(name.into(), id.into(), child_manifest);
        Ok(self)
    }

    /// Shared inline-component push. Both Agent and Skill children carry an
    /// AagentConfig, so the inline kind is always aagent.
    fn push_inline(&mut self, name: String, id: String, child_manifest: Manifest) {
        let child_cfg = match child_manifest.config {
            PackageConfig::Aagent(c) => InlineConfig::Aagent(c),
            _ => unreachable!("aagent/skill child always has AagentConfig"),
        };
        let inline = InlineComponent {
            name,
            id,
            kind: InlineComponentKind::Aagent,
            config: child_cfg,
            components: child_manifest.components,
            files: child_manifest.files,
            permissions: child_manifest.permissions,
            dependencies: child_manifest.dependencies,
        };
        self.inner
            .manifest
            .components
            .get_or_insert_with(Vec::new)
            .push(ComponentItem::Inline(inline));
    }

    /// Append an external ref component (any kind: mcp/atool tool, or aagent).
    pub fn ref_component(mut self, ns_name_at_version: impl Into<String>) -> Self {
        self.inner
            .manifest
            .components
            .get_or_insert_with(Vec::new)
            .push(ComponentItem::Ref(RefComponent {
                ref_target: ns_name_at_version.into(),
            }));
        self
    }

    /// Set install.flatten merge rules.
    pub fn flatten(mut self, f: InstallFlatten) -> Self {
        self.inner
            .manifest
            .install
            .get_or_insert_with(InstallBlock::default)
            .flatten = Some(f);
        self
    }

    common_builder_methods!();
}

// ---------------------------------------------------------------------------
// Skill (a prompt-only aagent)
// ---------------------------------------------------------------------------

/// Builder for reusable-prompt "skill" packages. A skill is reusable prompt
/// text — EE has no standalone skill kind, so this emits `kind = aagent` whose
/// only content is `system_prompt`.
pub struct Skill {
    inner: Inner,
}

impl Skill {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        let config = PackageConfig::Aagent(AagentConfig {
            system_prompt: String::new(),
            allowed_tools: None,
            model: None,
            history_limit: None,
            prompt_mode: None,
        });
        Self {
            inner: Inner::new(name, version, Kind::Aagent, config),
        }
    }

    fn cfg(&mut self) -> &mut AagentConfig {
        match &mut self.inner.manifest.config {
            PackageConfig::Aagent(c) => c,
            _ => unreachable!("Skill builder always holds an AagentConfig"),
        }
    }

    pub fn system_prompt(mut self, s: impl Into<String>) -> Self {
        self.cfg().system_prompt = s.into();
        self
    }

    pub fn system_prompt_from_file(mut self, path: impl AsRef<Path>) -> Result<Self> {
        let s = std::fs::read_to_string(path.as_ref())?;
        self.cfg().system_prompt = s;
        Ok(self)
    }

    pub fn allowed_tools(mut self, t: Vec<String>) -> Self {
        self.cfg().allowed_tools = Some(t);
        self
    }

    /// Preferred model backend id (EE `config.model`). Replaces v1 `.model_hint()`.
    pub fn model(mut self, m: impl Into<String>) -> Self {
        self.cfg().model = Some(m.into());
        self
    }

    common_builder_methods!();
}
