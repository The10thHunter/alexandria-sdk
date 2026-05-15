//! Fluent builders mirroring the TypeScript SDK.
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
    AgentConfig, ComponentItem, Dependency, FileEntry, InlineComponent, InlineComponentKind,
    InlineConfig, InstallBlock, InstallFlatten, K8sResources, Kind, Manifest, PackageConfig,
    Permissions, RefComponent, SkillConfig, ToolConfig, ToolK8sTransport, ToolTransport,
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
    fn new(name: impl Into<String>, version: impl Into<String>, kind: Kind, config: PackageConfig) -> Self {
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
        self.manifest.permissions.get_or_insert_with(Permissions::default)
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
            self.inner.manifest.files.get_or_insert_with(Vec::new).push(f);
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
// Tool
// ---------------------------------------------------------------------------

/// Builder for `kind: tool` packages.
pub struct Tool {
    inner: Inner,
}

impl Tool {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        let config = PackageConfig::Tool(ToolConfig {
            binary: String::new(),
            default_port: None,
            transport: None,
            args: None,
            k8s_image: None,
            k8s_capabilities: None,
            k8s_port: None,
            k8s_transport: None,
            k8s_resources: None,
            k8s_min_warm: None,
            k8s_idle_timeout_seconds: None,
        });
        Self { inner: Inner::new(name, version, Kind::Tool, config) }
    }

    fn cfg(&mut self) -> &mut ToolConfig {
        match &mut self.inner.manifest.config {
            PackageConfig::Tool(c) => c,
            _ => unreachable!("Tool builder always holds a ToolConfig"),
        }
    }

    pub fn binary(mut self, p: impl Into<String>) -> Self {
        self.cfg().binary = p.into();
        self
    }
    pub fn port(mut self, p: u16) -> Self {
        self.cfg().default_port = Some(p);
        self
    }
    pub fn transport(mut self, t: ToolTransport) -> Self {
        self.cfg().transport = Some(t);
        self
    }
    pub fn args(mut self, a: Vec<String>) -> Self {
        self.cfg().args = Some(a);
        self
    }
    pub fn k8s_image(mut self, img: impl Into<String>) -> Self {
        self.cfg().k8s_image = Some(img.into());
        self
    }
    pub fn k8s_capabilities(mut self, c: Vec<String>) -> Self {
        self.cfg().k8s_capabilities = Some(c);
        self
    }
    pub fn k8s_port(mut self, p: u16) -> Self {
        self.cfg().k8s_port = Some(p);
        self
    }
    pub fn k8s_transport(mut self, t: ToolK8sTransport) -> Self {
        self.cfg().k8s_transport = Some(t);
        self
    }
    pub fn k8s_resources(mut self, r: K8sResources) -> Self {
        self.cfg().k8s_resources = Some(r);
        self
    }
    pub fn k8s_min_warm(mut self, n: u32) -> Self {
        self.cfg().k8s_min_warm = Some(n);
        self
    }
    pub fn k8s_idle_timeout(mut self, seconds: u32) -> Self {
        self.cfg().k8s_idle_timeout_seconds = Some(seconds);
        self
    }

    common_builder_methods!();
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

/// Builder for `kind: agent` packages.
pub struct Agent {
    inner: Inner,
}

impl Agent {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        let config = PackageConfig::Agent(AgentConfig {
            system_prompt: String::new(),
            allowed_tools: None,
            llm: None,
            history_limit: None,
        });
        Self { inner: Inner::new(name, version, Kind::Agent, config) }
    }

    fn cfg(&mut self) -> &mut AgentConfig {
        match &mut self.inner.manifest.config {
            PackageConfig::Agent(c) => c,
            _ => unreachable!("Agent builder always holds an AgentConfig"),
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

    /// Set config.llm (replaces v1 `.model()`). Freeform preferred LLM id.
    pub fn llm(mut self, m: impl Into<String>) -> Self {
        self.cfg().llm = Some(m.into());
        self
    }

    pub fn history_limit(mut self, n: u32) -> Self {
        self.cfg().history_limit = Some(n);
        self
    }

    /// Append an inline sub-agent to components[].
    /// `name` is the local label; `id` is the canonical ns/name@version.
    pub fn component(mut self, name: impl Into<String>, id: impl Into<String>, child: Agent) -> Result<Self> {
        let child_manifest = child.build()?;
        let child_cfg = match child_manifest.config {
            PackageConfig::Agent(c) => InlineConfig::Agent(c),
            _ => unreachable!("Agent child always has AgentConfig"),
        };
        let inline = InlineComponent {
            name: name.into(),
            id: id.into(),
            kind: InlineComponentKind::Agent,
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
        Ok(self)
    }

    /// Append an inline sub-skill to components[].
    pub fn component_skill(mut self, name: impl Into<String>, id: impl Into<String>, child: Skill) -> Result<Self> {
        let child_manifest = child.build()?;
        let child_cfg = match child_manifest.config {
            PackageConfig::Skill(c) => InlineConfig::Skill(c),
            _ => unreachable!("Skill child always has SkillConfig"),
        };
        let inline = InlineComponent {
            name: name.into(),
            id: id.into(),
            kind: InlineComponentKind::Skill,
            config: child_cfg,
            components: None,
            files: child_manifest.files,
            permissions: child_manifest.permissions,
            dependencies: child_manifest.dependencies,
        };
        self.inner
            .manifest
            .components
            .get_or_insert_with(Vec::new)
            .push(ComponentItem::Inline(inline));
        Ok(self)
    }

    /// Append an external ref component (any kind: tool, skill, or agent).
    pub fn ref_component(mut self, ns_name_at_version: impl Into<String>) -> Self {
        self.inner
            .manifest
            .components
            .get_or_insert_with(Vec::new)
            .push(ComponentItem::Ref(RefComponent { ref_target: ns_name_at_version.into() }));
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
// Skill
// ---------------------------------------------------------------------------

/// Builder for `kind: skill` packages.
pub struct Skill {
    inner: Inner,
}

impl Skill {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        let config = PackageConfig::Skill(SkillConfig {
            system_prompt: String::new(),
            allowed_tools: None,
            llm: None,
            tags: None,
        });
        Self { inner: Inner::new(name, version, Kind::Skill, config) }
    }

    fn cfg(&mut self) -> &mut SkillConfig {
        match &mut self.inner.manifest.config {
            PackageConfig::Skill(c) => c,
            _ => unreachable!("Skill builder always holds a SkillConfig"),
        }
    }

    pub fn system_prompt(mut self, s: impl Into<String>) -> Self {
        self.cfg().system_prompt = s.into();
        self
    }

    pub fn allowed_tools(mut self, t: Vec<String>) -> Self {
        self.cfg().allowed_tools = Some(t);
        self
    }

    /// Set config.llm (replaces v1 `.model_hint()`). Freeform preferred LLM id.
    pub fn llm(mut self, m: impl Into<String>) -> Self {
        self.cfg().llm = Some(m.into());
        self
    }

    pub fn tags(mut self, t: Vec<String>) -> Self {
        self.cfg().tags = Some(t);
        self
    }

    common_builder_methods!();
}
