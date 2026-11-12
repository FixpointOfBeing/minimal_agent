use std::env;


use crate::{
    AgentError, Config, InputItem,
    build_system_prompt,
    tools::{ToolRegistry, ToolSpec},
};


use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Context {
    pub cwd: PathBuf,
    pub config: Config,
    pub registry: ToolRegistry,
    pub history: Vec<InputItem>,
    pub system_prompt: String,
    pub tool_specs: Vec<ToolSpec>,
}

impl Context {
    pub fn new() -> Result<Context, AgentError> {
        match env::current_dir() {
            Ok(cwd) => {
                let registry = ToolRegistry::new();
                let tool_specs = registry.tool_specs();
                let system_prompt = build_system_prompt(&cwd, &tool_specs);
                let config = Config::new()?;
                let history = vec![];
                Ok(Context { cwd, config, registry, system_prompt, history, tool_specs })
            },
            Err(_) => Err(AgentError::new("failed to get the current working directory")),
        }
    }
    /// Resolve `raw` against `cwd` for a mutating tool, confining the result to
    /// the project root.
    ///
    /// Absolute paths and `..` escapes that land outside `cwd` are rejected.
    /// Symlinks are resolved on the nearest existing ancestor, so a link inside
    /// the project cannot redirect a write to an unrelated location.
    pub fn resolve_for_mutation(&self, raw: &str) -> Result<PathBuf, String> {
        let cwd = self.cwd.as_path();
        if raw.is_empty() {
            return Err("path must not be empty".to_string());
        }

        let candidate =
            if Path::new(raw).is_absolute() { PathBuf::from(raw) } else { cwd.join(raw) };

        let canonical_cwd = fs::canonicalize(cwd).map_err(|error| {
            format!("cannot resolve working directory {}: {error}", cwd.display())
        })?;

        // Walk up from the candidate to its nearest existing ancestor and
        // canonicalize it, so symlinks resolve to their real location. The
        // non-existing tail is re-attached afterwards.
        let mut tail: Vec<OsString> = Vec::new();
        let mut current = candidate.as_path();
        let base = loop {
            match fs::canonicalize(current) {
                Ok(real) => break real,
                Err(_) => {
                    let name =
                        current.file_name().ok_or_else(|| format!("cannot resolve path: {raw}"))?;
                    tail.push(name.to_os_string());
                    match current.parent() {
                        Some(parent) if !parent.as_os_str().is_empty() => current = parent,
                        _ => return Err(format!("cannot resolve path: {raw}")),
                    }
                },
            }
        };

        // Reattach the tail, collapsing any `.`/`..` components that slipped through.
        let mut resolved = base;
        for component in tail.iter().rev() {
            match component.to_str() {
                Some(".") => {},
                Some("..") => {
                    resolved.pop();
                },
                _ => resolved.push(component),
            }
        }

        if !resolved.starts_with(&canonical_cwd) {
            return Err(format!("refusing to write outside the project root: {raw}"));
        }

        Ok(resolved)
    }
}
