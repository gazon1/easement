//! `suggest`: what the dependency graph says about the policy, which of its
//! suggestions rest on unread files, the outcome of `suggest --check`, and
//! the patch of `suggest --patch`.

use std::fs;

use easement::commands::{self, Outcome};

use crate::common::{fixture, git_in, project};
use crate::run::{cache, config};

#[test]
fn suggest_check_fails_on_unused_allowances_only() {
    let policy = |allow: &str| {
        format!(
            "version: 1
components:
  app: {{ in: src/app }}
  lib: {{ in: src/lib }}
  old: {{ in: src/old }}

rules:
  app:
    may_depend_on: [{allow}]
"
        )
    };
    let files = |policy: String| {
        project(&[
            ("easement.yml", &policy),
            (
                "src/app/a.ts",
                "import { l } from \"../lib/l\";\nexport const a = l;\n",
            ),
            ("src/lib/l.ts", "export const l = 1;\n"),
            ("src/old/o.ts", "export const o = 1;\n"),
        ])
    };
    let (_tmp, root) = files(policy("lib, old"));
    let found = commands::suggest(&config(&root), &mut cache()).unwrap();
    assert_eq!(found.outcome(true), Outcome::Failed);
    assert!(found.text().contains("app -> old"), "{}", found.text());
    // Without --check the same report is only informative.
    assert_eq!(found.outcome(false), Outcome::Passed);
    let (_tmp, root) = files(policy("lib"));
    let found = commands::suggest(&config(&root), &mut cache()).unwrap();
    assert_eq!(found.outcome(true), Outcome::Passed);
}

/// `suggest --format json` gives each suggestion as data, and an item next to
/// files no indexer read says how many: what an agent must not act on blindly.
#[test]
fn suggest_says_in_json_which_suggestions_rest_on_unread_files() {
    let (_tmp, root) = project(&[
        ("go.mod", "module example.com/m\n\ngo 1.22\n"),
        (
            "easement.yml",
            "version: 1\ncomponents:\n  app: { in: app }\n  lib: { in: lib }\n  log: { in: log }\nrules:\n  app: { may_depend_on: [lib, log] }\n",
        ),
        ("app/main.go", "package app\n\nfunc Run() {}\n"),
        // Built only with the `race` tag: no indexer reads it, and it uses lib.
        (
            "app/race.go",
            "//go:build race\n\npackage app\n\nimport \"example.com/m/lib\"\n\nfunc R() { lib.L() }\n",
        ),
        ("lib/lib.go", "package lib\n\nfunc L() {}\n"),
        ("log/log.go", "package log\n\nfunc P() {}\n"),
    ]);
    let found = commands::suggest(&config(&root), &mut cache()).unwrap();
    assert_eq!(found.outcome(false), Outcome::Passed);
    let report = serde_json::to_value(&found).unwrap();
    assert_eq!(
        report["unreliable"],
        serde_json::json!([
            { "from": "app", "to": "lib", "unread": { "app": 1 } },
            { "from": "app", "to": "log", "unread": { "app": 1 } },
        ]),
        "{report}"
    );
    assert_eq!(report["removable"], serde_json::json!([]), "{report}");
    assert_eq!(
        report["evidence_gaps"][0]["files"],
        serde_json::json!(["app/race.go"])
    );
}

#[test]
fn suggest_reports_what_the_graph_says_about_the_config() {
    let (_tmp, root) = fixture("ts");
    let found = commands::suggest(&config(&root), &mut cache()).unwrap();
    assert_eq!(found.outcome(false), Outcome::Passed);
    let said = found.text();
    assert!(said.contains("Depended on but not allowed"), "{said}");
    assert!(said.contains("app: infra (1 dependency(ies))"), "{said}");
}

/// The patch names the config by its path in the project, however the config
/// was given (the binary passes `--config` as written, here it is absolute),
/// so `git apply` in the root of the project applies it.
#[test]
fn suggest_patch_drops_unused_allowances_and_keeps_the_rest() {
    let policy = "version: 1
# who may use whom
components:
  app: { in: src/app }
  lib: { in: src/lib }
  old: { in: src/old }

rules:
  app:
    may_depend_on:
      - lib # the only one used
      - { name: old, because: was needed once }
";
    let (_tmp, root) = project(&[
        ("easement.yml", policy),
        (
            "src/app/a.ts",
            "import { l } from \"../lib/l\";\nexport const a = l;\n",
        ),
        ("src/lib/l.ts", "export const l = 1;\n"),
        ("src/old/o.ts", "export const o = 1;\n"),
    ]);
    let config = config(&root);
    let diff = commands::suggest_patch(&config, &mut cache()).unwrap().diff;
    assert!(
        diff.contains("--- a/easement.yml\n+++ b/easement.yml\n"),
        "{diff}"
    );
    assert!(
        diff.contains("-      - { name: old, because: was needed once }"),
        "{diff}"
    );
    assert!(!diff.contains("-      - lib"), "{diff}");

    fs::write(root.join("fix.patch"), &diff).unwrap();
    let applied = git_in(&root).args(["apply", "fix.patch"]).status().unwrap();
    assert!(applied.success());
    let fixed = fs::read_to_string(&config).unwrap();
    assert!(fixed.contains("# who may use whom") && fixed.contains("- lib # the only one used"));
    assert!(!fixed.contains("old,"), "{fixed}");
    let found = commands::suggest(&config, &mut cache()).unwrap();
    assert_eq!(found.outcome(true), Outcome::Passed);
    assert!(!found.text().contains("never imported"), "{}", found.text());
    // Nothing left to remove: an empty patch.
    assert_eq!(
        commands::suggest_patch(&config, &mut cache()).unwrap().diff,
        ""
    );
}
