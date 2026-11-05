//! A project file is named by a `/`-separated path relative to the project root.

use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

/// The extension of the file, `""` without one: `ts` of `src/a.d.ts`.
pub fn extension(path: &str) -> &str {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
}

/// The directory of the file, `""` at the root.
pub fn parent(path: &str) -> &str {
    Path::new(path)
        .parent()
        .and_then(|dir| dir.to_str())
        .unwrap_or("")
}

/// The name of the file, the last part of its path.
pub fn file_name(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

/// `/`-separated string form of a relative path, used for matching and output.
pub fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Matches each pattern and everything below it; `key` names the patterns in an error.
pub fn glob_set(patterns: &[String], key: &str) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let p = p.trim_end_matches('/');
        builder.add(Glob::new(p).with_context(|| format!("bad {key} pattern `{p}`"))?);
        builder.add(Glob::new(&format!("{p}/**"))?);
    }
    Ok(builder.build()?)
}

/// Project walker: honours .gitignore (even outside a git checkout) and
/// always skips the third-party directories of a toolchain (`node_modules`).
pub fn walker(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder.require_git(false).filter_entry(|e| {
        !e.file_name()
            .to_str()
            .is_some_and(crate::lang::registry::is_third_party)
    });
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_project_path() {
        assert_eq!(extension("src/a.d.ts"), "ts");
        assert_eq!(extension("Makefile"), "");
        assert_eq!(extension("v1.2/Makefile"), "");
        assert_eq!(parent("src/web/a.ts"), "src/web");
        assert_eq!(parent("a.ts"), "");
        assert_eq!(file_name("src/web/a.ts"), "a.ts");
        assert_eq!(file_name("a.ts"), "a.ts");
    }
}
