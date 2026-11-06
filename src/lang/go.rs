//! Go beyond its index: which files are programs of their own, and the build
//! tags the indexer builds with.

/// Whether the text of a Go file puts it in `package main`: a program of its own.
pub fn program(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .find(|l| l.starts_with("package "))
        .is_some_and(|l| l == "package main")
}

/// Whether `tag` is a build tag `go build -tags` takes: letters, digits, `_` and `.`.
pub fn build_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// The environment that has the indexer build with `tags`: `-tags` added to the
/// `GOFLAGS` easement runs with.
pub fn tags_env(tags: &[String]) -> (&'static str, String) {
    let mut flags = std::env::var("GOFLAGS").unwrap_or_default();
    if !flags.is_empty() {
        flags.push(' ');
    }
    flags.push_str(&format!("-tags={}", tags.join(",")));
    ("GOFLAGS", flags)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_build_tag_is_letters_digits_underscores_and_dots() {
        for tag in ["race", "e2e_1", "go1.22"] {
            assert!(super::build_tag(tag), "{tag}");
        }
        for tag in ["a b", "a,b", "", "-x"] {
            assert!(!super::build_tag(tag), "{tag}");
        }
    }

    #[test]
    fn a_program_is_in_package_main() {
        assert!(super::program("// tool\npackage main\n\nfunc main() {}\n"));
        assert!(!super::program("package mainly\n"));
        assert!(!super::program("package app\n"));
    }
}
