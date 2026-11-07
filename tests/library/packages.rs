//! The dependencies the manifests declare (`Cargo.toml`, `package.json`,
//! `go.mod`) judged as packages, what a manifest that cannot be read leaves
//! out, and the parts a manifest produces (a Cargo feature) that a component
//! owns.

use std::fs;
use std::path::PathBuf;

use easement::commands::Outcome;
use tempfile::TempDir;

use crate::common::fixture;
use crate::run::{check, gap_messages, json, leaves_out, text};

/// The manifests of a Cargo workspace are stored as `Cargo.toml.fixture`: tools that
/// walk the repository would take a `Cargo.toml` for a crate.
fn cargo_packages_fixture() -> (TempDir, PathBuf) {
    cargo_workspace_fixture("cargo_packages")
}

fn cargo_workspace_fixture(name: &str) -> (TempDir, PathBuf) {
    let (tmp, root) = fixture(name);
    for entry in ignore::WalkBuilder::new(&root).build().flatten() {
        if entry.file_name() == "Cargo.toml.fixture" {
            fs::rename(entry.path(), entry.path().with_file_name("Cargo.toml")).unwrap();
        }
    }
    (tmp, root)
}

#[test]
fn packages_check_the_dependencies_a_cargo_manifest_declares() {
    let (_tmp, root) = cargo_packages_fixture();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains("crates/chunk-store/Cargo.toml:7: store -> external axum is forbidden"),
        "{report}"
    );
    assert!(
        report.contains(
            "crates/web/Cargo.toml:7: web -> kit is not allowed (written as crates/testkit/Cargo.toml)"
        ),
        "{report}"
    );
    // `testkit` is a dev-dependency of chunk-store: the tests', not the crate's.
    assert!(
        !report.contains("crates/chunk-store/Cargo.toml:10"),
        "{report}"
    );
    assert!(report.contains("2 violation(s)"), "{report}");
}

/// A run reads each workspace manifest once; the next run reads it again.
#[test]
fn a_workspace_manifest_edited_between_runs_is_read_afresh() {
    let (_tmp, root) = cargo_packages_fixture();
    assert!(text(&check(&root)).contains("store -> external axum is forbidden"));
    let workspace = root.join("Cargo.toml");
    let manifest = fs::read_to_string(&workspace).unwrap();
    fs::write(
        &workspace,
        manifest.replace(
            "axum = \"0.7\"",
            "axum = { package = \"tower\", version = \"0.4\" }",
        ),
    )
    .unwrap();
    let report = text(&check(&root));
    assert!(!report.contains("axum is forbidden"), "{report}");
    assert!(report.contains("1 violation(s)"), "{report}");
}

#[test]
fn a_manifest_packages_cannot_read_is_said_not_passed_as_clean() {
    let (_tmp, root) = cargo_packages_fixture();
    fs::write(root.join("crates/web/Cargo.toml"), "[dependencies\nx = ").unwrap();
    let report = json(&check(&root));
    let partial = gap_messages(&report, "packages");
    assert_eq!(partial.len(), 1, "{partial:?}");
    assert!(
        partial[0]
            .as_str()
            .unwrap()
            .contains("crates/web/Cargo.toml")
    );
    assert!(leaves_out(&report), "{report}");
    let gap = &report["evidence_gaps"][0];
    assert_eq!(gap["kind"], "packages", "{gap}");
    assert_eq!(gap["files"], serde_json::json!(["crates/web/Cargo.toml"]));
    assert_eq!(gap["message"], partial[0]);
}

#[test]
fn packages_and_manifests_together_add_no_unjudged_toml() {
    let (_tmp, root) = cargo_packages_fixture();
    let config = fs::read_to_string(root.join("easement.yml")).unwrap();
    fs::write(
        root.join("easement.yml"),
        format!("{config}\nmanifests: true\n"),
    )
    .unwrap();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(!report.contains(".toml files"), "{report}");
    assert!(report.contains("2 violation(s)"), "{report}");
}

/// The feature that must stay out of a release build: only the web build and the tests
/// may turn it on, and no crate does by default.
#[test]
fn a_cargo_feature_is_a_part_that_a_component_owns() {
    let (_tmp, root) = cargo_workspace_fixture("cargo_features");
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains("crates/bad/Cargo.toml:6: bad -> fakes: fakes is open only to web, tests"),
        "{report}"
    );
    assert!(
        report
            .contains("crates/core/Cargo.toml:6: core -> fakes: fakes is open only to web, tests"),
        "{report}"
    );
    assert!(
        report.contains("because: the fake channel never gets into a release build"),
        "{report}"
    );
    assert!(!report.contains("crates/web/Cargo.toml:"), "{report}");
    assert!(!report.contains("crates/telegram/Cargo.toml:"), "{report}");
    assert!(report.contains("2 violation(s)"), "{report}");
}

/// `must_depend_on` is met by a dependency on a part that a component owns.
#[test]
fn a_part_that_a_component_owns_meets_must_depend_on() {
    let (_tmp, root) = cargo_workspace_fixture("cargo_features");
    let config = fs::read_to_string(root.join("easement.yml")).unwrap();
    let edited = config.replace(
        "core: { may_depend_on: [fakes] }",
        "core: { may_depend_on: [fakes], must_depend_on: [fakes] }",
    );
    assert_ne!(config, edited);
    fs::write(root.join("easement.yml"), edited).unwrap();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains("crates/core/Cargo.toml:6: core -> fakes"),
        "{report}"
    );
    assert!(report.contains("2 violation(s)"), "{report}");
}

/// A `part` that no manifest produces is a typo in the policy: it would own nothing
/// and be judged by no rule, so it fails as `unmatched` at its line, like a pattern
/// that matches no file, and names the part it likely means.
#[test]
fn a_part_that_matches_no_part_is_an_unmatched_violation() {
    let (_tmp, root) = cargo_workspace_fixture("cargo_features");
    let config = fs::read_to_string(root.join("easement.yml")).unwrap();
    let typo = config.replace("part: feature/fake", "part: feature/fakee");
    assert_ne!(config, typo);
    fs::write(root.join("easement.yml"), &typo).unwrap();
    let line = typo
        .lines()
        .position(|l| l.contains("part: feature/fakee"))
        .unwrap()
        + 1;
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = json(&checked);
    let unmatched: Vec<&serde_json::Value> = report["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["rule"] == "unmatched")
        .collect();
    assert_eq!(unmatched.len(), 1, "{report}");
    assert_eq!(unmatched[0]["file"], "easement.yml");
    assert_eq!(unmatched[0]["line"], line);
    let message = unmatched[0]["message"].as_str().unwrap();
    assert!(
        message.contains("`fakes`")
            && message.contains("feature/fakee")
            && message.contains("did you mean `feature/fake`?"),
        "{message}"
    );
    // A typo in the policy is no evidence gap.
    assert!(gap_messages(&report, "parts").is_empty(), "{report}");
    assert_eq!(report["evidence_gaps"], serde_json::json!([]));

    // `optional: true` accepts a part that may own nothing.
    fs::write(
        root.join("easement.yml"),
        typo.replace(
            "    part: feature/fakee\n",
            "    part: feature/fakee\n    optional: true\n",
        ),
    )
    .unwrap();
    let report = json(&check(&root));
    assert!(
        !report["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["rule"] == "unmatched"),
        "{report}"
    );
    assert!(gap_messages(&report, "parts").is_empty(), "{report}");
}

#[test]
fn packages_check_the_dependencies_a_package_json_declares() {
    let (_tmp, root) = fixture("npm_packages");
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains("packages/store/package.json:5: store -> external axios is forbidden"),
        "{report}"
    );
    assert!(
        report.contains(
            "packages/web/package.json:5: web -> kit is not allowed (written as packages/testkit/package.json)"
        ),
        "{report}"
    );
    // `@acme/testkit` is a devDependency of store: the tests', not the package's.
    assert!(
        !report.contains("packages/store/package.json:8"),
        "{report}"
    );
    assert!(report.contains("2 violation(s)"), "{report}");
}

#[test]
fn packages_check_the_modules_a_go_mod_requires() {
    let (_tmp, root) = fixture("go_packages");
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains("app/go.mod:7: app -> external github.com/lib/pq is forbidden"),
        "{report}"
    );
    assert!(
        report
            .contains("app/go.mod:8: app -> internal is not allowed (written as internal/go.mod)"),
        "{report}"
    );
    assert!(report.contains("2 violation(s)"), "{report}");
}

#[test]
fn packages_and_manifests_together_judge_a_package_json_once() {
    let (_tmp, root) = fixture("npm_packages");
    let config = fs::read_to_string(root.join("easement.yml")).unwrap();
    fs::write(
        root.join("easement.yml"),
        format!("{config}\nmanifests: true\n"),
    )
    .unwrap();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(!report.contains(".json files"), "{report}");
    assert!(report.contains("2 violation(s)"), "{report}");
}

#[test]
fn two_package_jsons_that_go_by_one_name_are_said() {
    let (_tmp, root) = fixture("npm_packages");
    fs::create_dir_all(root.join("packages/copy")).unwrap();
    fs::write(
        root.join("packages/copy/package.json"),
        "{\n  \"name\": \"@acme/api\"\n}\n",
    )
    .unwrap();
    let report = json(&check(&root));
    let partial = gap_messages(&report, "packages");
    assert_eq!(partial.len(), 1, "{partial:?}");
    assert!(partial[0].as_str().unwrap().contains("@acme/api"));
    assert!(leaves_out(&report), "{report}");
}

#[test]
fn a_go_mod_that_cannot_be_read_is_said_not_passed_as_clean() {
    let (_tmp, root) = fixture("go_packages");
    fs::write(
        root.join("app/go.mod"),
        "module example.com/app\n\nrequire (\n\texample.com/lib v1.0.0\n",
    )
    .unwrap();
    let report = json(&check(&root));
    let partial = gap_messages(&report, "packages");
    assert_eq!(partial.len(), 1, "{partial:?}");
    assert!(partial[0].as_str().unwrap().contains("app/go.mod"));
}
