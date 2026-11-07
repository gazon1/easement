//! A name one component re-exports from another (#103): `gw` may depend on `chunk`
//! and `chunk` on `blob`, but `gw` not on `blob`. A name of `blob` that `gw` writes
//! through `chunk` is a dependency on `blob`; the lines of `chunk` that refer to
//! `blob` are evidence of it too, so a change to the re-export alone is judged.

use std::fs;
use std::path::PathBuf;

use easement::CacheUse;
use easement::commands::{self, CheckOptions, Outcome};
use tempfile::TempDir;

use crate::common::{fixture, git, on_feature, project, write_all};
use crate::diff::diff_main;
use crate::languages::THREE;
use crate::run::{cache_in, check, check_with, composer_install, config, json, pinned, text};

/// A project of the components `blob`, `chunk` and `gw` in the directories under
/// `dir`, of `files`, committed on `main` and checked out on `feature`.
pub(crate) fn three(dir: &str, files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let policy = THREE
        .replace("BLOB", &format!("{dir}blob"))
        .replace("CHUNK", &format!("{dir}chunk"))
        .replace("GW", &format!("{dir}gw"));
    let mut all = vec![("easement.yml", policy.as_str())];
    all.extend_from_slice(files);
    let (tmp, root) = project(&all);
    on_feature(&root);
    (tmp, root)
}

/// A Cargo workspace of `blob`, `chunk` and `gw`, with the sources of `chunk` and
/// `gw` as given.
pub(crate) fn rust(chunk: &str, gw: &str) -> (TempDir, PathBuf) {
    let manifest = |name: &str, deps: &str| {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}"
        )
    };
    three(
        "",
        &[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"gw\", \"chunk\", \"blob\"]\nresolver = \"2\"\n",
            ),
            ("blob/Cargo.toml", &manifest("blob", "")),
            (
                "blob/src/lib.rs",
                "pub struct Block {\n    pub len: usize,\n}\n",
            ),
            (
                "chunk/Cargo.toml",
                &manifest("chunk", "blob = { path = \"../blob\" }\n"),
            ),
            ("chunk/src/lib.rs", chunk),
            (
                "gw/Cargo.toml",
                &manifest("gw", "chunk = { path = \"../chunk\" }\n"),
            ),
            ("gw/src/lib.rs", gw),
        ],
    )
}

/// Rewrites `file` of `root` to `text` and commits it on the branch checked out.
pub(crate) fn change(root: &std::path::Path, file: &str, text: &str) {
    fs::write(root.join(file), text).unwrap();
    git(root, &["commit", "-q", "-am", "change"]);
}

/// The violations of a report as (file, line, from -> to).
pub(crate) fn found(report: &serde_json::Value) -> Vec<(String, u64, String)> {
    report["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["file"].as_str().unwrap().to_string(),
                v["line"].as_u64().unwrap(),
                format!(
                    "{} -> {}",
                    v["from"].as_str().unwrap(),
                    v["to"].as_str().unwrap()
                ),
            )
        })
        .collect()
}

/// `chunk` uses `blob::Block` in `make()` above the line that turns its own `Block`
/// into a re-export, so the first line of `chunk` that refers to `blob` is not the
/// one that changed.
#[test]
fn a_rust_re_export_changed_alone_is_judged_in_a_diff_run_and_named() {
    let (_tmp, root) = rust(
        "pub fn make() -> blob::Block {\n    blob::Block { len: 1 }\n}\n\npub struct Block;\n",
        "pub fn named(_: chunk::Block) {}\n",
    );
    change(
        &root,
        "chunk/src/lib.rs",
        "pub fn make() -> blob::Block {\n    blob::Block { len: 1 }\n}\n\npub use blob::Block;\n",
    );
    let checked = check_with(&root, diff_main);
    let report = json(&checked);
    assert_eq!(checked.outcome(), Outcome::Failed, "{report}");
    assert_eq!(
        found(&report),
        [("gw/src/lib.rs".to_string(), 1, "gw -> blob".to_string())],
        "{report}"
    );
    let violation = &report["violations"][0];
    assert_eq!(violation["reexported_by"], serde_json::json!(["chunk"]));
    assert!(
        violation["evidence"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!({ "file": "chunk/src/lib.rs", "line": 5 })),
        "{violation}"
    );
    assert!(
        text(&checked).contains(
            "gw/src/lib.rs:1: gw -> blob is not allowed (written as blob/src/lib.rs, re-exported by chunk)"
        ),
        "{}",
        text(&checked)
    );
}

/// The one violation of a diff run in which only the re-exporter changed: at
/// `file:line` of `gw`, naming `chunk`, with the line of `chunk` that changed
/// among its evidence.
fn judged_through_chunk(root: &std::path::Path, file: &str, line: u64, changed: (&str, u64)) {
    let checked = check_with(root, diff_main);
    let report = json(&checked);
    assert_eq!(checked.outcome(), Outcome::Failed, "{report}");
    assert_eq!(
        found(&report),
        [(file.to_string(), line, "gw -> blob".to_string())],
        "{report}"
    );
    let violation = &report["violations"][0];
    assert_eq!(
        violation["reexported_by"],
        serde_json::json!(["chunk"]),
        "{violation}"
    );
    assert!(
        violation["evidence"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!({ "file": changed.0, "line": changed.1 })),
        "{violation}"
    );
}

/// A glob re-export names only the module it re-exports, not the name.
#[test]
fn a_typescript_glob_re_export_changed_alone_is_judged_in_a_diff_run() {
    let (_tmp, root) = three(
        "src/",
        &[
            ("src/blob/index.ts", "export class Block {\n  len = 1;\n}\n"),
            (
                "src/chunk/index.ts",
                "import { Block as B } from \"../blob\";\n\nexport const make = (): B => new B();\n\nexport class Block {}\n",
            ),
            (
                "src/gw/named.ts",
                "import { Block } from \"../chunk\";\n\nexport const named = (b: Block) => b;\n",
            ),
        ],
    );
    change(
        &root,
        "src/chunk/index.ts",
        "import { Block as B } from \"../blob\";\n\nexport const make = (): B => new B();\n\nexport * from \"../blob\";\n",
    );
    judged_through_chunk(&root, "src/gw/named.ts", 1, ("src/chunk/index.ts", 5));
}

#[test]
fn a_python_name_an_init_re_exports_changed_alone_is_judged_in_a_diff_run() {
    let (_tmp, root) = three(
        "app/",
        &[
            ("app/__init__.py", ""),
            ("app/blob/__init__.py", "class Block:\n    len = 1\n"),
            (
                "app/chunk/__init__.py",
                "from app.blob import Block as B\n\n\ndef make():\n    return B()\n\n\nclass Block:\n    pass\n",
            ),
            (
                "app/gw/__init__.py",
                "from app.chunk import Block\n\n\ndef named(b: Block):\n    return b\n",
            ),
        ],
    );
    change(
        &root,
        "app/chunk/__init__.py",
        "from app.blob import Block as B\n\n\ndef make():\n    return B()\n\n\nfrom app.blob import Block\n",
    );
    judged_through_chunk(&root, "app/gw/__init__.py", 1, ("app/chunk/__init__.py", 8));
}

/// TypeScript files of `gw` that use `Block` of `blob`: `named.ts` through the
/// barrel of `chunk`, `direct.ts` from `blob` itself.
fn typescript_both() -> (TempDir, PathBuf) {
    three(
        "src/",
        &[
            ("src/blob/index.ts", "export class Block {\n  len = 1;\n}\n"),
            (
                "src/chunk/index.ts",
                "export { Block } from \"../blob\";\nexport const own = 1;\n",
            ),
            (
                "src/gw/named.ts",
                "import { Block } from \"../chunk\";\n\nexport const named = (b: Block) => b;\n",
            ),
            (
                "src/gw/direct.ts",
                "import { Block } from \"../blob\";\n\nexport const direct = (b: Block) => b;\n",
            ),
        ],
    )
}

/// The full check reports each use once; only the one through `chunk` names it.
#[test]
fn a_full_check_names_the_re_exporter_only_of_a_name_written_through_it() {
    let (_tmp, root) = typescript_both();
    // Indexed afresh, so that no other run decides whether the report says it is cached.
    let own = TempDir::new().unwrap();
    let checked = commands::check(
        &CheckOptions::new(config(&root)),
        &mut cache_in(own.path(), CacheUse::Off),
    )
    .unwrap();
    let report = json(&checked);
    assert_eq!(
        found(&report),
        [
            ("src/gw/direct.ts".to_string(), 1, "gw -> blob".to_string()),
            ("src/gw/named.ts".to_string(), 1, "gw -> blob".to_string()),
        ],
        "{report}"
    );
    // `direct.ts` names no re-exporter and has its own line alone as evidence;
    // `named.ts` names `chunk`, with the barrel's line.
    pinned(|| {
        insta::assert_json_snapshot!("check_reexport_json", report["violations"]);
        insta::assert_snapshot!("check_reexport_text", text(&checked));
    });
}

/// A baseline written before the re-exporter was named still legalizes the violation.
#[test]
fn a_baselined_dependency_through_a_re_export_stays_legalized() {
    let (_tmp, root) = typescript_both();
    fs::write(
        root.join("easement.baseline.yml"),
        "violations:\n- rule: dependency\n  file: src/gw/direct.ts\n  target: src/blob/index.ts\n  from: gw\n  to: blob\n- rule: dependency\n  file: src/gw/named.ts\n  target: src/blob/index.ts\n  from: gw\n  to: blob\n",
    )
    .unwrap();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Passed, "{}", text(&checked));
    assert!(text(&checked).contains("2 legalized"), "{}", text(&checked));
}

/// The lines of `chunk` stand behind the violation of `gw`, so a check limited to
/// `chunk` reports it too, as it reports a cycle that `chunk` is part of.
#[test]
fn a_check_limited_to_the_re_exporter_reports_the_violation_it_stands_behind() {
    let (_tmp, root) = typescript_both();
    let report = json(&check_with(&root, |o| o.components = vec!["chunk".into()]));
    assert_eq!(
        found(&report),
        [("src/gw/named.ts".to_string(), 1, "gw -> blob".to_string())],
        "{report}"
    );
}

/// A glob import of `chunk` names its module on its own line, never on the line
/// that writes the name.
#[test]
fn a_re_export_used_through_a_glob_import_is_judged_in_a_diff_run() {
    let (_tmp, root) = rust(
        "pub fn make() -> blob::Block {\n    blob::Block { len: 1 }\n}\n\npub struct Block;\n",
        "use chunk::*;\n\npub fn named(_: Block) {}\n",
    );
    change(
        &root,
        "chunk/src/lib.rs",
        "pub fn make() -> blob::Block {\n    blob::Block { len: 1 }\n}\n\npub use blob::Block;\n",
    );
    judged_through_chunk(&root, "gw/src/lib.rs", 3, ("chunk/src/lib.rs", 5));
}

/// A barrel that re-exports the name itself, as the full check sees it above.
#[test]
fn a_typescript_barrel_re_export_changed_alone_is_judged_in_a_diff_run() {
    let (_tmp, root) = three(
        "src/",
        &[
            ("src/blob/index.ts", "export class Block {\n  len = 1;\n}\n"),
            (
                "src/chunk/index.ts",
                "import { Block as B } from \"../blob\";\n\nexport const make = (): B => new B();\n\nexport class Block {}\n",
            ),
            (
                "src/gw/named.ts",
                "import { Block } from \"../chunk\";\n\nexport const named = (b: Block) => b;\n",
            ),
        ],
    );
    change(
        &root,
        "src/chunk/index.ts",
        "import { Block as B } from \"../blob\";\n\nexport const make = (): B => new B();\n\nexport { Block } from \"../blob\";\n",
    );
    judged_through_chunk(&root, "src/gw/named.ts", 1, ("src/chunk/index.ts", 5));
}

/// A module alias leaves no occurrence of `blob` (`from app import blob as b`), so
/// the line of `b.Block` names no module; `gw` also imports something of `chunk`,
/// which depends on `blob`, but re-exports nothing that `gw` uses.
#[test]
fn a_name_written_through_a_module_alias_names_no_re_exporter() {
    let (_tmp, root) = three(
        "app/",
        &[
            ("app/__init__.py", ""),
            ("app/blob/__init__.py", "class Block:\n    len = 1\n"),
            (
                "app/chunk/__init__.py",
                "from app.blob import Block\n\nown = Block()\n",
            ),
            (
                "app/gw/__init__.py",
                "from app import blob as b\nfrom app.chunk import own\n\n\ndef named(x: b.Block):\n    return own\n",
            ),
        ],
    );
    let report = json(&check(&root));
    let through_chunk: Vec<&serde_json::Value> = report["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["reexported_by"] != serde_json::Value::Null)
        .collect();
    assert_eq!(through_chunk, Vec::<&serde_json::Value>::new(), "{report}");
    assert!(
        found(&report)
            .iter()
            .any(|(file, _, pair)| file == "app/gw/__init__.py" && pair == "gw -> blob"),
        "{report}"
    );
}

/// A PHP class import names only a module, as a glob import does, but brings in no
/// names: `Order` of `domain` stands on a line of its own, and `domain` refers to
/// `infra`, yet the function of `infra` that `web` calls comes from `infra` itself.
#[test]
fn a_class_import_beside_a_function_import_names_no_re_exporter() {
    let (_tmp, root) = fixture("php");
    write_all(
        &root,
        &[
            (
                "composer.json",
                "{\n  \"autoload\": { \"psr-4\": { \"App\\\\\": \"src/\" }, \"files\": [\"src/Infra/functions.php\"] }\n}\n",
            ),
            (
                "src/Infra/functions.php",
                "<?php\nnamespace App\\Infra;\n\nfunction connect(): int { return 1; }\n",
            ),
            (
                "src/Domain/Order.php",
                "<?php\nnamespace App\\Domain;\n\nuse App\\Infra\\OrderRepository;\n\nclass Order { public ?OrderRepository $repo = null; }\n",
            ),
            (
                "src/Web/Status.php",
                "<?php\nnamespace App\\Web;\n\nuse App\\Domain\\Order;\nuse function App\\Infra\\connect;\n\nclass Status { public function up(Order $o): int { return connect(); } }\n",
            ),
        ],
    );
    composer_install(&root);
    let report = json(&check(&root));
    assert!(
        found(&report)
            .iter()
            .any(|(file, _, pair)| file == "src/Web/Status.php" && pair == "web -> infra"),
        "{report}"
    );
    let named: Vec<&serde_json::Value> = report["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["reexported_by"] != serde_json::Value::Null)
        .collect();
    assert_eq!(named, Vec::<&serde_json::Value>::new(), "{report}");
}
