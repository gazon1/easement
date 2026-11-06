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

/// `name` in the directory `dir`, `""` being the root.
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// `rel`, a `/`-separated path written in `dir`, relative to the root: `.` and `..`
/// resolved by the letters alone. `None` if it climbs above the root.
pub fn resolve(dir: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in rel.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// Whether `path` is the directory `dir` or below it; every path is below the root `""`.
pub fn is_under(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
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

    #[test]
    fn joins_and_resolves_within_the_project() {
        assert_eq!(join("", "go.mod"), "go.mod");
        assert_eq!(join("svc/api", "go.mod"), "svc/api/go.mod");
        assert_eq!(resolve("web/src", "../lib/./a"), Some("web/lib/a".into()));
        assert_eq!(resolve("", "./a"), Some("a".into()));
        assert_eq!(resolve("web", "../.."), None);
        assert!(is_under("src/a.ts", ""));
        assert!(is_under("src", "src"));
        assert!(is_under("src/a.ts", "src"));
        assert!(!is_under("srcs/a.ts", "src"));
    }
}
