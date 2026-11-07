//! The components of a policy and the files they match: a component that
//! matches no file, an optional one, files `target_only` keeps from being
//! checked as sources, and the README that documents every key of the policy.

use std::fs;
use std::path::{Path, PathBuf};

use easement::commands::{self, Outcome};
use tempfile::TempDir;

use crate::common::{commit_all, project};
use crate::run::{cache, check, check_with, config, json, text};

/// A project whose `ghost` component points at a directory that is gone.
fn ghost_project() -> (TempDir, PathBuf) {
    project(&[
        (
            "easement.yml",
            "version: 1\ncomponents:\n  app: { in: src/app }\n  ghost: { in: src/renamed_away }\nrules:\n  app: { may_depend_on: [ghost] }\n",
        ),
        ("src/app/Main.kt", "package app\n"),
    ])
}

#[test]
fn a_component_that_matches_no_file_fails() {
    let (_tmp, root) = ghost_project();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let said = text(&checked);
    assert!(
        said.contains("easement.yml:4: component `ghost` matches no file (in: src/renamed_away)"),
        "{said}"
    );
    assert!(said.contains("[unmatched]"), "{said}");
}

#[test]
fn an_optional_component_may_match_no_file() {
    let (_tmp, root) = project(&[
        (
            "easement.yml",
            "version: 1\ntests: auto\ncomponents:\n  app: { in: src/app }\n  plugins: { in: src/plugins, optional: true }\nrules:\n  app: { may_depend_on: [plugins] }\n",
        ),
        ("src/app/Main.kt", "package app\n"),
    ]);
    // Neither `plugins` nor the `tests` of `tests: auto` has a file.
    assert_eq!(check(&root).outcome(), Outcome::Passed);
}

#[test]
fn an_unmatched_component_is_reported_in_json() {
    let (_tmp, root) = ghost_project();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = json(&checked);
    let v = &report["violations"][0];
    assert_eq!(v["rule"], "unmatched");
    assert_eq!(v["file"], "easement.yml");
    assert_eq!(v["from"], "ghost");
    assert_eq!(v["target"], "src/renamed_away");
}

#[test]
fn diff_and_scope_still_report_an_unmatched_component() {
    let (_tmp, root) = ghost_project();
    commit_all(&root);
    // The pull request touches neither the config nor a line with an import.
    fs::write(root.join("src/app/Other.kt"), "package app\n").unwrap();
    let checked = check_with(&root, |o| o.diff = Some("main".into()));
    assert_eq!(checked.outcome(), Outcome::Failed);
    assert!(
        text(&checked).contains("component `ghost` matches no file"),
        "{}",
        text(&checked)
    );
    let checked = check_with(&root, |o| o.components = vec!["app".into()]);
    assert_eq!(checked.outcome(), Outcome::Failed);
    assert!(
        text(&checked).contains("component `ghost` matches no file"),
        "{}",
        text(&checked)
    );
}

#[test]
fn the_baseline_never_accepts_an_unmatched_component() {
    let (_tmp, root) = ghost_project();
    let recorded = commands::baseline(&config(&root), false, &mut cache()).unwrap();
    assert_eq!(recorded.outcome(), Outcome::Passed);
    let said = recorded.text();
    assert!(said.contains("0 violation(s) written"), "{said}");
    assert!(
        said.contains("1 violation(s) of the config left out"),
        "{said}"
    );
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    assert!(
        text(&checked).contains("component `ghost` matches no file"),
        "{}",
        text(&checked)
    );
}

/// Files that .gitignore hides are not read and no import of them is seen,
/// so a component of nothing else checks nothing.
#[test]
fn a_component_of_ignored_files_matches_no_file() {
    let (_tmp, root) = project(&[
        (
            "easement.yml",
            "version: 1\ncomponents:\n  app: { in: src/app }\n  gen: { in: src/gen }\nrules:\n  app: { may_depend_on: [gen] }\n",
        ),
        (".gitignore", "src/gen/\n"),
        ("src/gen/api.ts", "export const api = 1;\n"),
        (
            "src/app/a.ts",
            "import { api } from \"../gen/api\";\nexport const a = api;\n",
        ),
    ]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let said = text(&checked);
    assert!(said.contains("component `gen` matches no file"), "{said}");
    assert!(said.contains(".gitignore hide do not count"), "{said}");
}

#[test]
fn target_only_files_are_imported_but_never_checked_as_sources() {
    let policy = |key: &str| {
        format!(
            "version: 1
components:
  app: {{ in: src/app }}
  generated: {{ in: src/generated }}
rules:
  app: {{ may_depend_on: [] }}
  generated: {{ may_depend_on: [] }}
{key}: ['**/*.gen.ts']
"
        )
    };
    let files = |policy: String| {
        project(&[
            ("easement.yml", &policy),
            ("tsconfig.json", "{\"include\": [\"src\"]}\n"),
            (
                "src/app/a.ts",
                "import { api } from \"../generated/api.gen\";\nexport const a = api;\n",
            ),
            (
                "src/generated/api.gen.ts",
                "import { a } from \"../app/a\";\nexport const api = 1;\nexport const b = a;\n",
            ),
        ])
    };
    let pairs = |root: &Path| -> Vec<(String, String)> {
        json(&check(root))["violations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| {
                (
                    v["file"].as_str().unwrap().to_string(),
                    v["to"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    // The import of the generated file is seen and judged; its own import of the app is not.
    let (_tmp, root) = files(policy("target_only"));
    assert_eq!(
        pairs(&root),
        [("src/app/a.ts".to_string(), "generated".to_string())]
    );
    // `exclude` hides the file altogether: importing it fails as `excluded`,
    // and the component that owned it matches no file.
    let (_tmp, root) = files(policy("exclude"));
    assert_eq!(
        pairs(&root),
        [
            ("easement.yml".to_string(), String::new()),
            ("src/app/a.ts".to_string(), String::new())
        ]
    );
}

#[test]
fn readme_mentions_every_config_key_of_the_schema() {
    let schema: serde_json::Value = serde_json::from_str(&easement::schema_text()).unwrap();
    let readme = include_str!("../../README.md");
    let mut missing = Vec::new();
    let mut keys = |properties: &serde_json::Value| {
        for key in properties["properties"].as_object().unwrap().keys() {
            if !readme.contains(&format!("{key}:")) && !readme.contains(&format!("`{key}`")) {
                missing.push(key.clone());
            }
        }
    };
    keys(&schema);
    keys(&schema["definitions"]["Component"]);
    keys(&schema["definitions"]["Rule"]);
    assert!(missing.is_empty(), "README never mentions: {missing:?}");
}
