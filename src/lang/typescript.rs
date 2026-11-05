//! TypeScript and JavaScript beyond their index: how the bundlers resolve a
//! relative specifier, and the uses of a file that an index may not report.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

use super::registry::TYPESCRIPT;
use crate::relpath::parent;

/// The file a relative specifier names, as the bundlers resolve it: the path
/// itself, with an extension, or its index.
pub fn resolve(from_dir: &str, spec: &str, files: &BTreeSet<&str>) -> Option<String> {
    let mut parts: Vec<&str> = if from_dir.is_empty() {
        Vec::new()
    } else {
        from_dir.split('/').collect()
    };
    for segment in spec.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    let base = parts.join("/");
    let exts: Vec<&str> = TYPESCRIPT
        .extensions
        .iter()
        .copied()
        .chain(["d.ts"])
        .collect();
    std::iter::once(base.clone())
        .chain(exts.iter().map(|e| format!("{base}.{e}")))
        .chain(exts.iter().map(|e| format!("{base}/index.{e}")))
        .find(|candidate| files.contains(candidate.as_str()))
}

/// The files `file` reaches through an import for its side effect
/// (`import "./polyfill"`), `require` and a literal `import()` of a relative
/// path, which an index may not report as a use.
pub fn unindexed_uses(text: &str, file: &str, files: &BTreeSet<&str>) -> Vec<String> {
    static RELATIVE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"(?m)(?:^\s*import\s+|\brequire\s*\(\s*|\bimport\s*\(\s*)["'](\.{1,2}/[^"']+)["']"#,
        )
        .expect("valid regex")
    });
    RELATIVE
        .captures_iter(text)
        .filter_map(|capture| resolve(parent(file), &capture[1], files))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_specifiers_like_a_bundler() {
        let files: BTreeSet<&str> = ["src/a.ts", "src/lib/index.ts", "src/p.d.ts"].into();
        assert_eq!(
            resolve("src/x", "../a", &files).as_deref(),
            Some("src/a.ts")
        );
        assert_eq!(
            resolve("src", "./lib", &files).as_deref(),
            Some("src/lib/index.ts")
        );
        assert_eq!(resolve("src", "./p", &files).as_deref(), Some("src/p.d.ts"));
        assert_eq!(resolve("", "../../a", &files), None);
    }
}
