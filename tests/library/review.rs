//! `review`: what a change to the policy since the merge-base with a base
//! revision permits, forbids and accepts over the current code, the files it
//! moves, opens or closes, and the gaps of both scans it rests on.

use std::fs;
use std::path::{Path, PathBuf};

use easement::commands::{self, Outcome};
use tempfile::TempDir;

use crate::common::{WEB_AND_STORAGE, commit_all, fixture, git, project, web_and_storage};
use crate::run::{cache, check, config, parse};

/// The review of the change to the policy of the project at `root` since
/// `main` as text, as `review --base main` prints it.
fn review_text(root: &Path) -> String {
    commands::review(&config(root), "main", &mut cache())
        .unwrap()
        .text()
}

/// The same review as data, as `review --base main --format json` prints it.
fn review_json(root: &Path) -> serde_json::Value {
    parse(
        &commands::review(&config(root), "main", &mut cache())
            .unwrap()
            .json(),
    )
}

/// Two components that do not import each other, committed on `main` and
/// checked out on `feature`.
fn web_and_unused_storage(policy: &str) -> (TempDir, PathBuf) {
    let (tmp, root) = project(&[
        ("easement.yml", policy),
        ("src/web/page.ts", "export const page = 1;\n"),
        ("src/storage/db.ts", "export const save = 1;\n"),
        ("src/storage/internal/pool.ts", "export const pool = 1;\n"),
    ]);
    commit_all(&root);
    git(&root, &["checkout", "-q", "-b", "feature"]);
    (tmp, root)
}

const UNUSED_STORAGE: &str = "version: 1
components:
  web: { in: src/web }
  storage: { in: src/storage }
rules:
  web: { may_depend_on: [] }
";

/// A grant that no import uses yet produces no violation, yet the review
/// names it with its reason.
#[test]
fn review_names_a_permission_no_code_uses() {
    let (_tmp, root) = web_and_unused_storage(UNUSED_STORAGE);
    fs::write(
        config(&root),
        UNUSED_STORAGE.replace(
            "web: { may_depend_on: [] }",
            "web: { may_depend_on: [{ name: storage, because: pages save drafts }] }",
        ),
    )
    .unwrap();
    assert_eq!(check(&root).outcome(), Outcome::Passed);
    let said = review_text(&root);
    assert!(
        said.contains("web -> storage newly permitted (may_depend_on)"),
        "{said}"
    );
    assert!(said.contains("because: pages save drafts"), "{said}");
    let report = review_json(&root);
    assert_eq!(
        report["permissions"],
        serde_json::json!([{
            "from": "web",
            "to": "storage",
            "change": "permitted",
            "clause": "may_depend_on",
            "because": "pages save drafts",
        }]),
        "{report}"
    );

    // A plain item gives the reason of its rule.
    fs::write(
        config(&root),
        UNUSED_STORAGE.replace(
            "web: { may_depend_on: [] }",
            "web: { may_depend_on: [storage], because: pages render what storage holds }",
        ),
    )
    .unwrap();
    let said = review_text(&root);
    assert!(
        said.contains(
            "web -> storage newly permitted (may_depend_on)\n    because: pages render what storage holds\n",
        ),
        "{said}"
    );
}

/// What the change to the policy makes of the code as it is: violations it
/// introduces and those it removes, a rule moved to `warn` told apart.
#[test]
fn review_reports_the_policy_delta_on_the_current_code() {
    let legacy = WEB_AND_STORAGE.replace("common: [storage]\n", "");
    let (_tmp, root) = web_and_storage(WEB_AND_STORAGE);
    fs::write(
        config(&root),
        WEB_AND_STORAGE.replace(
            "web: { may_depend_on: [] }",
            "web: { must_not_depend_on: [{ name: storage, because: pages go through the api }] }",
        ),
    )
    .unwrap();
    let report = review_json(&root);
    let introduced: Vec<(&str, u64, &str)> = report["delta"]["introduced"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["file"].as_str().unwrap(),
                v["line"].as_u64().unwrap(),
                v["because"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        introduced,
        [("src/web/page.ts", 1, "pages go through the api")],
        "{report}"
    );
    assert_eq!(
        report["delta"]["removed"],
        serde_json::json!([]),
        "{report}"
    );
    assert_eq!(report["rescanned"], serde_json::Value::Null, "{report}");
    let said = review_text(&root);
    assert!(
        said.contains("web -> storage newly rejected (must_not_depend_on)"),
        "{said}"
    );
    assert!(
        said.contains("Policy delta on the current code:\n  introduced (1):\n    src/web/page.ts:1: web -> storage is forbidden"),
        "{said}"
    );

    // Moved to `warn`, a violation fails no more: it leaves the delta as a warning.
    git(&root, &["checkout", "-q", "main"]);
    fs::write(config(&root), &legacy).unwrap();
    git(&root, &["commit", "-q", "-am", "legacy"]);
    git(&root, &["checkout", "-q", "-B", "feature"]);
    fs::write(config(&root), format!("{legacy}warn: [dependency]\n")).unwrap();
    let report = review_json(&root);
    assert_eq!(
        report["delta"]["removed"],
        serde_json::json!([]),
        "{report}"
    );
    assert_eq!(
        report["delta"]["warned"][0]["file"], "src/web/page.ts",
        "{report}"
    );
    assert_eq!(
        report["debt"]["warn_added"],
        serde_json::json!(["dependency"]),
        "{report}"
    );
}

/// When the change alters what is scanned, the current code is scanned again
/// under the old policy, and the review says so.
#[test]
fn review_scans_again_when_the_policy_changes_what_is_scanned() {
    let legacy = WEB_AND_STORAGE.replace("common: [storage]\n", "");
    let (_tmp, root) = web_and_storage(&legacy);
    fs::write(config(&root), format!("{legacy}exclude: [src/web]\n")).unwrap();
    let report = review_json(&root);
    assert_eq!(
        report["rescanned"],
        serde_json::json!(["exclude"]),
        "{report}"
    );
    assert_eq!(
        report["ownership"]["excluded"],
        serde_json::json!(["src/web/page.ts"]),
        "{report}"
    );
    assert_eq!(
        report["ownership"]["moved"],
        serde_json::json!([]),
        "{report}"
    );
    assert_eq!(
        report["delta"]["removed"][0]["file"], "src/web/page.ts",
        "{report}"
    );
    assert_eq!(
        report["debt"]["exclude_added"],
        serde_json::json!(["src/web"]),
        "{report}"
    );
    let said = review_text(&root);
    assert!(
        said.contains(
            "Scanned twice: the change alters what is scanned (`exclude`), so the current code was scanned again under the policy at the merge-base",
        ),
        "{said}"
    );
    assert!(
        said.contains("  newly excluded (1):\n    src/web/page.ts\n"),
        "{said}"
    );
}

/// Files whose component changes are listed with where they were and are.
#[test]
fn review_lists_files_that_change_component() {
    let (_tmp, root) = web_and_unused_storage(UNUSED_STORAGE);
    fs::write(
        config(&root),
        UNUSED_STORAGE
            .replace(
                "  storage: { in: src/storage }",
                "  storage: { in: src/storage/db.ts }\n  pool: { in: src/storage/internal }",
            )
            .replace("  web: { in: src/web }\n", "")
            .replace("rules:\n  web: { may_depend_on: [] }\n", ""),
    )
    .unwrap();
    let report = review_json(&root);
    assert_eq!(
        report["ownership"]["moved"],
        serde_json::json!([
            { "file": "src/storage/internal/pool.ts", "from": "storage", "to": "pool" },
            { "file": "src/web/page.ts", "from": "web", "to": null },
        ]),
        "{report}"
    );
    assert_eq!(
        report["components"],
        serde_json::json!({ "added": ["pool"], "removed": ["web"] }),
        "{report}"
    );
    let said = review_text(&root);
    assert!(
        said.contains("  storage -> pool (1):\n    src/storage/internal/pool.ts\n"),
        "{said}"
    );
    assert!(
        said.contains("  web -> no component (1):\n    src/web/page.ts\n"),
        "{said}"
    );
    assert!(
        said.contains("Components added: pool\nComponents removed: web\n"),
        "{said}"
    );
}

/// The delta rests on both scans: code either could not read is an evidence
/// gap, and code the new policy accepts unread is an accepted gap.
#[test]
fn review_reports_the_gaps_of_both_policies() {
    let (_tmp, root) = fixture("uncovered");
    commit_all(&root);
    let policy = fs::read_to_string(config(&root)).unwrap();
    // Excluded now, the files only the old scan read stay a gap of the delta.
    fs::write(config(&root), format!("{policy}exclude: [tools]\n")).unwrap();
    let report = review_json(&root);
    assert_eq!(
        report["rescanned"],
        serde_json::json!(["exclude"]),
        "{report}"
    );
    let gaps = report["evidence_gaps"].as_array().unwrap();
    assert_eq!(gaps.len(), 1, "{report}");
    assert_eq!(
        gaps[0]["files"],
        serde_json::json!(["tools/gen.go", "tools/other.go"]),
        "{report}"
    );

    fs::write(
        config(&root),
        format!("{policy}uncovered_ok: [{{ name: tools/gen.go, because: generated }}]\n"),
    )
    .unwrap();
    let report = review_json(&root);
    assert_eq!(
        report["accepted_gaps"][0]["files"],
        serde_json::json!(["tools/gen.go"]),
        "{report}"
    );
    // The old policy did not accept it, so the delta read nothing of it.
    assert_eq!(
        report["evidence_gaps"][0]["files"],
        serde_json::json!(["tools/gen.go", "tools/other.go"]),
        "{report}"
    );
    let said = review_text(&root);
    assert!(said.contains("Accepted gaps (`uncovered_ok`"), "{said}");
    assert!(
        said.contains("Evidence gaps (the delta and the file lists say nothing about this code):"),
        "{said}"
    );
}

/// A policy input outside the repository has no past: the review cannot see
/// how it changed, and says so.
#[test]
fn review_names_a_policy_input_whose_past_is_unknown() {
    let (tmp, root) = project(&[
        (
            "shared.yml",
            "components:\n  storage: { in: repo/src/storage }\n",
        ),
        (
            "repo/easement.yml",
            "extends: ../shared.yml\nversion: 1\ncomponents:\n  web: { in: src/web }\n  storage: { in: src/storage }\n",
        ),
        ("repo/src/web/page.ts", "export const page = 1;\n"),
    ]);
    let root = root.join("repo");
    fs::rename(tmp.path().join("tsconfig.json"), root.join("tsconfig.json")).unwrap();
    commit_all(&root);
    let report = review_json(&root);
    let unknown = report["inputs"]["past_unknown"].as_array().unwrap();
    assert!(
        unknown.len() == 1 && unknown[0].as_str().unwrap().ends_with("shared.yml"),
        "{report}"
    );
    let said = review_text(&root);
    assert!(
        said.contains("shared.yml is outside the repository, so its content at the merge-base is unknown and how it changed is not reviewed"),
        "{said}"
    );
}

/// A wider surface is stated as the patterns it adds, with the files of
/// today that it opens.
#[test]
fn review_lists_the_files_a_wider_surface_opens() {
    let policy = UNUSED_STORAGE.replace(
        "storage: { in: src/storage }",
        "storage: { in: src/storage, public: src/storage/db.ts }",
    );
    let (_tmp, root) = web_and_unused_storage(&policy);
    fs::write(
        config(&root),
        policy.replace(
            "public: src/storage/db.ts",
            "public: [src/storage/db.ts, src/storage/internal/**]",
        ),
    )
    .unwrap();
    let said = review_text(&root);
    assert!(
        said.contains(
            "  storage.public: added `src/storage/internal/**`\n    opens 1 file(s):\n      src/storage/internal/pool.ts\n",
        ),
        "{said}"
    );
    let report = review_json(&root);
    assert_eq!(
        report["surfaces"],
        serde_json::json!([{
            "component": "storage",
            "key": "public",
            "before": ["src/storage/db.ts"],
            "after": ["src/storage/db.ts", "src/storage/internal/**"],
            "added": ["src/storage/internal/**"],
            "removed": [],
            "opens": ["src/storage/internal/pool.ts"],
            "closes": [],
        }]),
        "{report}"
    );
}

/// Naming a surface on an open component closes every file outside it to
/// all, and opens what it holds only to its importers.
#[test]
fn review_lists_the_files_a_named_surface_closes() {
    let (_tmp, root) = web_and_unused_storage(UNUSED_STORAGE);
    fs::write(
        config(&root),
        UNUSED_STORAGE.replace(
            "storage: { in: src/storage }",
            "storage: { in: src/storage, surfaces: { pool: { in: src/storage/internal, importers: [web] } } }",
        ),
    )
    .unwrap();
    let said = review_text(&root);
    assert!(
        said.contains(
            "  storage.surfaces: set to `pool: src/storage/internal (open to web)` (was unset: no named surface)\n    closes 2 file(s):\n      src/storage/db.ts\n      src/storage/internal/pool.ts\n",
        ),
        "{said}"
    );
    let report = review_json(&root);
    assert_eq!(
        report["surfaces"],
        serde_json::json!([{
            "component": "storage",
            "key": "surfaces",
            "before": null,
            "after": ["pool: src/storage/internal (open to web)"],
            "added": ["pool: src/storage/internal (open to web)"],
            "removed": [],
            "opens": [],
            "closes": ["src/storage/db.ts", "src/storage/internal/pool.ts"],
        }]),
        "{report}"
    );
}

/// A baseline of legalized `web -> storage` imports from the given files.
fn baseline_of(files: impl IntoIterator<Item = String>) -> String {
    let mut out = String::from("violations:\n");
    for file in files {
        out.push_str(&format!(
            "- rule: dependency\n  file: {file}\n  target: src/storage/db.ts\n  from: web\n  to: storage\n"
        ));
    }
    out
}

/// Debt accepted in the baseline is listed entry by entry, however much of it.
#[test]
fn review_lists_every_baseline_entry_added_or_removed() {
    let (_tmp, root) = project(&[
        ("easement.yml", UNUSED_STORAGE),
        ("src/web/page.ts", "export const page = 1;\n"),
        ("src/storage/db.ts", "export const save = 1;\n"),
        (
            "easement.baseline.yml",
            &baseline_of(["src/web/old.ts".to_string()]),
        ),
    ]);
    commit_all(&root);
    let added: Vec<String> = (0..10).map(|i| format!("src/web/new{i}.ts")).collect();
    fs::write(
        root.join("easement.baseline.yml"),
        baseline_of(added.clone()),
    )
    .unwrap();

    let text = review_text(&root);
    assert!(text.contains("Baseline entries added (10):"), "{text}");
    for file in &added {
        assert!(
            text.contains(&format!(
                "{file}: web -> storage (src/storage/db.ts) [dependency]"
            )),
            "{text}"
        );
    }
    assert!(text.contains("Baseline entries removed (1):"), "{text}");
    let report = review_json(&root);
    let debt = &report["debt"];
    assert_eq!(
        debt["baseline_added"].as_array().unwrap().len(),
        10,
        "{report}"
    );
    assert_eq!(
        debt["baseline_removed"],
        serde_json::json!([{
            "rule": "dependency",
            "file": "src/web/old.ts",
            "target": "src/storage/db.ts",
            "from": "web",
            "to": "storage",
        }]),
        "{report}"
    );
}
