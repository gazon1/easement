//! Go beyond its index: which files are programs of their own, the imports a
//! file writes, and the build tags the indexer builds with.

use tree_sitter::{Node, Parser, Tree};

use super::Import;
use super::registry::Inherited;

/// The syntax tree of Go source. Any text has one: a syntax error is a node of it.
fn tree(text: &str) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .expect("the Go grammar matches the tree-sitter it is built with");
    parser
        .parse(text, None)
        .expect("a parse with no timeout and no cancellation ends with a tree")
}

/// The declarations at the head of a file, in order: its package clause, then its
/// imports, up to the first other declaration. Comments are skipped.
fn head<'t>(tree: &'t Tree) -> impl Iterator<Item = Node<'t>> {
    let root = tree.root_node();
    (0..root.named_child_count())
        .filter_map(move |i| root.named_child(i as u32))
        .filter(|node| node.kind() != "comment")
        .take_while(|node| matches!(node.kind(), "package_clause" | "import_declaration"))
}

/// The imports the text of a Go file writes, at the line of each path, as
/// spelled and resolved by nothing: those of its `import` declarations, single
/// and grouped, with any name, `.` or `_` before the path. cgo's `import "C"`
/// is no package, so it is left out.
pub fn written_imports(text: &str) -> Vec<Import> {
    let tree = tree(text);
    let mut imports = Vec::new();
    for declaration in head(&tree).filter(|node| node.kind() == "import_declaration") {
        let mut specs = Vec::new();
        collect_specs(declaration, &mut specs);
        imports.extend(specs.into_iter().filter_map(|spec| {
            let path = spec.child_by_field_name("path")?;
            let quoted = path.utf8_text(text.as_bytes()).ok()?;
            let specifier = quoted.get(1..quoted.len().checked_sub(1)?)?;
            (specifier != "C").then(|| Import {
                specifier: specifier.to_string(),
                line: path.start_position().row + 1,
            })
        }));
    }
    imports
}

/// The `import_spec` nodes below `node`, single or in an `import_spec_list`.
fn collect_specs<'t>(node: Node<'t>, specs: &mut Vec<Node<'t>>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "import_spec" => specs.push(child),
            "import_spec_list" => collect_specs(child, specs),
            _ => {}
        }
    }
}

/// Whether the text of a Go file puts it in `package main`: a program of its own.
pub fn program(text: &str) -> bool {
    let tree = tree(text);
    let Some(clause) = head(&tree).find(|node| node.kind() == "package_clause") else {
        return false;
    };
    let mut cursor = clause.walk();
    clause
        .named_children(&mut cursor)
        .find(|node| node.kind() == "package_identifier")
        .is_some_and(|name| name.utf8_text(text.as_bytes()) == Ok("main"))
}

/// Whether `tag` is a build tag `go build -tags` takes: letters, digits, `_` and `.`.
pub fn build_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// The environment that has the indexer build with `tags`: `-tags` added to the
/// `GOFLAGS` the indexer would inherit.
pub fn tags_env(tags: &[String], inherited: Inherited) -> (&'static str, String) {
    let mut flags = inherited("GOFLAGS").unwrap_or_default();
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

    /// `(line, module)` of each import `text` writes.
    fn written(text: &str) -> Vec<(usize, String)> {
        super::written_imports(text)
            .into_iter()
            .map(|import| (import.line, import.specifier))
            .collect()
    }

    #[test]
    fn written_imports_are_every_form_at_the_line_of_its_path() {
        let text = "/* License\n   import \"not/this\"\n*/\n//go:build linux\n\n// Package app.\npackage app\n\nimport \"fmt\"\nimport lb \"example.com/m/lib\"\n\nimport (\n\t// a comment, \"not/this\"\n\t. \"example.com/m/dot\" /* dot */\n\t_ \"strings\"\n\n\traw `example.com/m/raw`\n\t\"example.com/ext\" // trailing\n)\n\nimport (\"os\"; \"io\")\n\nfunc A() { _ = \"import \\\"x\\\"\" }\n";
        assert_eq!(
            written(text),
            [
                (9, "fmt"),
                (10, "example.com/m/lib"),
                (14, "example.com/m/dot"),
                (15, "strings"),
                (17, "example.com/m/raw"),
                (18, "example.com/ext"),
                (21, "os"),
                (21, "io"),
            ]
            .map(|(line, module)| (line, module.to_string()))
        );
    }

    #[test]
    fn a_byte_order_mark_hides_no_import() {
        let text = "\u{feff}package a\n\nimport _ \"example.com/repro/b\"\n";
        assert_eq!(written(text), [(3, "example.com/repro/b".to_string())]);
    }

    #[test]
    fn cgo_is_no_written_import() {
        let text =
            "package c\n\n// #include <stdio.h>\nimport \"C\"\n\nimport (\n\t\"C\"\n\t\"os\"\n)\n";
        assert_eq!(written(text), [(8, "os".to_string())]);
    }

    #[test]
    fn imports_end_at_the_first_other_declaration() {
        let text = "package a\n\nimport \"fmt\"\n\nvar s = `\nimport \"no\"\n`\n\nfunc f() {}\n";
        assert_eq!(written(text), [(3, "fmt".to_string())]);
        assert!(written("package a\n\nfunc f() {}\n").is_empty());
    }

    #[test]
    fn a_program_is_in_package_main() {
        assert!(super::program("// tool\npackage main\n\nfunc main() {}\n"));
        assert!(!super::program("package mainly\n"));
        assert!(!super::program("package app\n"));
        // The package clause is a token, not a line: an import comment may follow
        // it, and a comment before it is no clause.
        assert!(super::program(
            "package main // import \"example.com/tool\"\n"
        ));
        assert!(super::program("package/* a */main\n"));
        assert!(!super::program("/*\npackage main\n*/\npackage app\n"));
        // A byte order mark before the clause is no token, as for the compiler.
        assert!(super::program("\u{feff}package main\n"));
    }
}
