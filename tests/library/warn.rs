//! `warn`: the rules a policy reports without failing on them, and which rules
//! it may name.

use std::fs;
use std::path::{Path, PathBuf};

use easement::commands::{self, Outcome};
use tempfile::TempDir;

use crate::common::project;
use crate::reach::reach_project;
use crate::run::{cache, check, config, error, json, text, try_check_with};

/// `reach_project` with the rule `reach` reported as a warning.
fn warned_reach_project() -> (TempDir, PathBuf) {
    let (tmp, root) = reach_project(&[]);
    warn_on_reach(&root);
    (tmp, root)
}

fn warn_on_reach(root: &Path) {
    let config = fs::read_to_string(root.join("easement.yml")).unwrap();
    fs::write(
        root.join("easement.yml"),
        format!("{config}warn: [reach]\n"),
    )
    .unwrap();
}

#[test]
fn a_rule_listed_in_warn_reports_without_failing() {
    let (_tmp, root) = warned_reach_project();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Passed);
    let report = text(&checked);
    assert!(
        report.contains("Warnings (rules in `warn`; do not add more):"),
        "{report}"
    );
    assert!(
        report.contains("domain reaches http through util"),
        "{report}"
    );
    assert!(
        report.contains("easement passed: 0 violation(s), 0 stale, 0 legalized, 1 warning(s)"),
        "{report}"
    );
    let report = json(&checked);
    assert_eq!(report["violations"].as_array().unwrap().len(), 0);
    assert_eq!(report["warnings"][0]["rule"], "reach");
    let github = checked.report().github();
    assert!(
        github.contains("::warning file=src/domain/Order.kt,line=2,title=easement reach::"),
        "{github}"
    );
    let sarif = checked.report().sarif();
    assert!(sarif.contains("\"level\": \"warning\""), "{sarif}");
    // The baseline records only what fails.
    let recorded = commands::baseline(&config(&root), false, &mut cache()).unwrap();
    assert_eq!(recorded.outcome(), Outcome::Passed);
    assert!(
        recorded.text().contains("0 violation(s) written"),
        "{}",
        recorded.text()
    );
}

#[test]
fn moving_a_baselined_rule_into_warn_leaves_no_stale_entries() {
    let (_tmp, root) = reach_project(&[]);
    let recorded = commands::baseline(&config(&root), false, &mut cache()).unwrap();
    assert_eq!(recorded.outcome(), Outcome::Passed);
    warn_on_reach(&root);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Passed);
    assert!(text(&checked).contains("0 stale"), "{}", text(&checked));
}

#[test]
fn warn_takes_only_rules_that_can_be_tolerated() {
    let (_tmp, root) = project(&[
        (
            "easement.yml",
            "version: 1\ncomponents:\n  a: { in: a }\nwarn: [bogus]\n",
        ),
        ("a/A.kt", "package a\n"),
    ]);
    let refused = error(try_check_with(&root, |_| {}));
    assert!(
        refused.contains("unknown rule `bogus` in warn"),
        "{refused}"
    );
    assert!(refused.contains("dependency"), "{refused}");
    assert!(!refused.contains("unmatched"), "{refused}");
    fs::write(
        root.join("easement.yml"),
        "version: 1\ncomponents:\n  a: { in: a }\nwarn: [unmatched]\n",
    )
    .unwrap();
    let refused = error(try_check_with(&root, |_| {}));
    assert!(
        refused.contains("`unmatched` is about the config"),
        "{refused}"
    );
}

#[test]
fn warnings_are_sorted_and_warn_is_inherited_through_extends() {
    let (_tmp, root) = project(&[
        (
            "base.yml",
            "version: 1\ncomponents:\n  a: { in: src/a }\n  b: { in: src/b }\nwarn: [dependency]\n",
        ),
        ("easement.yml", "extends: base.yml\n"),
        ("src/a/Z.kt", "package a\nimport b.B\nclass Z\n"),
        (
            "src/a/A.kt",
            "package a\n\nimport b.B\nimport b.C\nclass A\n",
        ),
        ("src/b/B.kt", "package b\nclass B\n"),
        ("src/b/C.kt", "package b\nclass C\n"),
    ]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Passed);
    let out = text(&checked);
    let a3 = out.find("src/a/A.kt:3").unwrap();
    let a4 = out.find("src/a/A.kt:4").unwrap();
    let z2 = out.find("src/a/Z.kt:2").unwrap();
    assert!(a3 < a4 && a4 < z2, "{out}");
    // A `warn` of its own replaces the inherited one.
    fs::write(root.join("easement.yml"), "extends: base.yml\nwarn: []\n").unwrap();
    assert_eq!(check(&root).outcome(), Outcome::Failed);
}
