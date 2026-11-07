//! Files the check does not read: what `exclude` hides and `.gitignore` hides,
//! and an import of such a file, which fails as `excluded`.

use std::fs;

use easement::CacheUse;
use easement::commands::Outcome;
use tempfile::TempDir;

use crate::common::project;
use crate::run::{check, check_in, text};

#[test]
fn an_import_of_an_excluded_file_fails() {
    let config = "version: 1\ncomponents:\n  app: { in: src/app }\nrules:\n  app: { may_depend_on: [] }\nexclude: [src/legacy]\n";
    // Resolved by an indexer.
    let (_tmp, root) = project(&[
        ("easement.yml", config),
        ("src/legacy/api.ts", "export const api = 1;\n"),
        (
            "src/app/a.ts",
            "import { api } from \"../legacy/api\";\nexport const a = api;\n",
        ),
    ]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    assert!(
        text(&checked).contains(
            "src/app/a.ts:1: app depends on src/legacy/api.ts, which `exclude` hides, so no rule checks it; list it in `target_only` if it is generated, in a component or `unowned` if it is part of the project, else drop the dependency",
        ),
        "{}",
        text(&checked)
    );
    // Resolved by a tree-sitter adapter.
    let (_tmp, root) = project(&[
        ("easement.yml", config),
        ("src/legacy/Api.kt", "package legacy\nclass Api\n"),
        ("src/app/A.kt", "package app\nimport legacy.Api\nclass A\n"),
    ]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let said = text(&checked);
    assert!(
        said.contains("src/app/A.kt:2: app depends on src/legacy/Api.kt"),
        "{said}"
    );
    assert!(said.contains("[excluded]"), "{said}");
    assert!(!said.contains("[dependency]"), "{said}");
    // A deliberate exception is suppressed like any import.
    fs::write(
        root.join("src/app/A.kt"),
        "package app\n// easement:disable-next-line excluded -- legacy client, remove with #1\nimport legacy.Api\nclass A\n",
    )
    .unwrap();
    assert_eq!(check(&root).outcome(), Outcome::Passed);
}

#[test]
fn an_excluded_file_is_part_of_the_index_cache_key() {
    let config = "version: 1\ncomponents:\n  app: { in: src/app }\nrules:\n  app: { may_depend_on: [] }\nexclude: [src/gen]\n";
    let (_tmp, root) = project(&[
        ("easement.yml", config),
        ("src/gen/keep.ts", "export {};\n"),
        (
            "src/app/a.ts",
            "import { g } from \"../gen/g\";\nexport const a = g;\n",
        ),
    ]);
    let cache = TempDir::new().unwrap();
    let checked = check_in(&root, cache.path(), CacheUse::On).unwrap();
    assert_eq!(checked.outcome(), Outcome::Passed);
    // Code generation writes the file the app imports.
    fs::write(root.join("src/gen/g.ts"), "export const g = 1;\n").unwrap();
    let checked = check_in(&root, cache.path(), CacheUse::On).unwrap();
    assert_eq!(checked.outcome(), Outcome::Failed);
    assert!(text(&checked).contains("[excluded]"), "{}", text(&checked));
}

#[test]
fn excluded_wildcard_imports_and_ignored_files() {
    // A Kotlin wildcard import resolves to the directory.
    let (_tmp, root) = project(&[
        (
            "easement.yml",
            "version: 1\ncomponents:\n  app: { in: src/app }\nexclude: ['src/legacy/**']\n",
        ),
        ("src/legacy/Api.kt", "package legacy\nclass Api\n"),
        ("src/app/A.kt", "package app\nimport legacy.*\nclass A\n"),
    ]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    assert!(
        text(&checked).contains("src/app/A.kt:2: app depends on src/legacy"),
        "{}",
        text(&checked)
    );
    // A file that .gitignore hides is not read, in any language.
    let (_tmp, root) = project(&[
        (
            "easement.yml",
            "version: 1\ncomponents:\n  app: { in: src/app }\nexclude: [src/gen]\n",
        ),
        (".gitignore", "src/gen/\n"),
        ("src/gen/g.ts", "export const g = 1;\n"),
        (
            "src/app/a.ts",
            "import { g } from \"../gen/g\";\nexport const a = g;\n",
        ),
    ]);
    assert_eq!(check(&root).outcome(), Outcome::Passed);
}
