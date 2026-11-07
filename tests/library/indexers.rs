//! The indexers easement runs: what `doctor` says of them, and the settings
//! of `indexers:` that change what they read.

use std::fs;

use easement::Environment;
use easement::commands::{self, Outcome};

use crate::common::{fixture, project};
use crate::run::{check, config, parse, pinned, text};

#[test]
fn doctor_reports_the_indexer_of_each_language() {
    let (_tmp, root) = fixture("uncovered");
    let diagnosed = commands::doctor(&config(&root), &Environment::of_process()).unwrap();
    assert_eq!(diagnosed.outcome(), Outcome::Passed);
    let said = diagnosed.text();
    assert!(said.contains("go: scip-go"), "{said}");
    assert!(said.contains("ok"), "{said}");
    let report = parse(&diagnosed.json());
    assert_eq!(report["ok"], true);
    assert_eq!(report["languages"][0]["language"], "go");
    assert_eq!(report["languages"][0]["binary"], "scip-go");
    assert_eq!(report["languages"][0]["found"], true);
}

#[test]
fn doctor_says_when_dependencies_are_not_installed() {
    let (_tmp, root) = project(&[
        (
            "easement.yml",
            "version: 1\ncomponents:\n  app: { in: src }\n",
        ),
        (
            "package.json",
            r#"{ "dependencies": { "left-pad": "1.0.0" } }"#,
        ),
        ("src/a.ts", "export const a = 1;\n"),
    ]);
    let diagnosed = commands::doctor(&config(&root), &Environment::of_process()).unwrap();
    assert_eq!(diagnosed.outcome(), Outcome::Failed);
    assert!(
        diagnosed.text().contains("dependencies are not installed"),
        "{}",
        diagnosed.text()
    );
}

#[test]
fn indexer_settings_bring_go_files_behind_a_build_constraint_into_the_index() {
    let (_tmp, root) = fixture("gotags");
    // The race file is not built without its tag: no indexer reads it.
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Passed);
    let said = text(&checked);
    assert!(said.contains("  pkg/race.go"), "{said}");
    assert!(said.contains("1 files not covered by an indexer"), "{said}");
    let policy = fs::read_to_string(config(&root)).unwrap();
    fs::write(
        config(&root),
        format!("{policy}indexers:\n  go: {{ tags: [race] }}\n"),
    )
    .unwrap();
    // Same files, other tags: the cached index of the first run must not be used.
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Passed);
    assert!(
        !text(&checked).contains("not covered"),
        "{}",
        text(&checked)
    );
}

/// Without the indexer on the `PATH` of the environment it runs in, `doctor`
/// names the indexer and how to install it.
#[test]
fn doctor_names_a_missing_indexer_and_how_to_install_it() {
    let (_tmp, root) = fixture("uncovered");
    let diagnosed =
        commands::doctor(&config(&root), &Environment::of_process().with("PATH", "")).unwrap();
    assert_eq!(diagnosed.outcome(), Outcome::Failed);
    pinned(|| {
        insta::assert_snapshot!("doctor_missing_text", diagnosed.text());
        insta::assert_json_snapshot!("doctor_missing_json", parse(&diagnosed.json()));
    });
}
