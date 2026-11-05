//! Go beyond its index: which files are programs of their own.

/// Whether the text of a Go file puts it in `package main`: a program of its own.
pub fn program(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .find(|l| l.starts_with("package "))
        .is_some_and(|l| l == "package main")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_program_is_in_package_main() {
        assert!(super::program("// tool\npackage main\n\nfunc main() {}\n"));
        assert!(!super::program("package mainly\n"));
        assert!(!super::program("package app\n"));
    }
}
