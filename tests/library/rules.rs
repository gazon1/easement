//! The rules of a policy beyond the allowed dependencies: instances and their
//! isolation, `importers`, priority and reasons, unowned files, required
//! imports, files and directives, forbidden files and `tests_auto`; what
//! `--component` and `--scope` narrow; and a rule that names what does not exist.

use std::fs;
use std::path::Path;

use easement::commands::{self, CheckOptions, Outcome};

use crate::common::fixture;
use crate::run::{cache, check, check_with, config, error, json, text, try_check_with};

/// The violations of a check with the options `set` changes.
fn reports(root: &Path, set: impl FnOnce(&mut CheckOptions)) -> Vec<serde_json::Value> {
    json(&check_with(root, set))["violations"]
        .as_array()
        .unwrap()
        .clone()
}

fn find<'a>(found: &'a [serde_json::Value], file: &str, rule: &str) -> &'a serde_json::Value {
    found
        .iter()
        .find(|v| v["file"] == file && v["rule"] == rule)
        .unwrap_or_else(|| panic!("no {rule} in {file}: {found:?}"))
}

/// A rule that names a component the policy does not have stops the check with
/// an error that names it, as a policy file that does not exist does.
#[test]
fn config_errors_name_what_is_wrong() {
    let (_tmp, root) = fixture("go");
    fs::write(
        root.join("easement.yml"),
        "version: 1\ncomponents:\n  a: { in: a }\nrules:\n  a: { may_depend_on: [ghost] }\n",
    )
    .unwrap();
    let refused = error(try_check_with(&root, |_| {}));
    assert!(refused.contains("unknown component `ghost`"), "{refused}");
    error(try_check_with(&root, |o| {
        o.config = root.join("missing.yml")
    }));
}

#[test]
fn instances_importers_priority_reasons_and_unowned() {
    let (_tmp, root) = fixture("structure");
    let found = reports(&root, |_| {});
    assert_eq!(found.len(), 7, "{found:?}");

    // Instances of `pages` must not depend on each other.
    let isolation = find(&found, "src/pages/contacts/index.ts", "isolation");
    assert!(
        isolation["message"]
            .as_str()
            .unwrap()
            .contains("src/pages/contacts -> src/pages/segments")
    );
    assert_eq!(isolation["because"], "pages ship as separate bundles");

    // `ee` is imported only from wiring, even though `common` lets everyone in.
    let importers = find(&found, "src/pages/segments/list.ts", "dependency");
    assert!(
        importers["message"]
            .as_str()
            .unwrap()
            .contains("ee is open only to wiring")
    );
    assert_eq!(importers["because"], "ee is licensed separately");

    // The priority of `tests` beats the longer pattern of `billing`.
    let test = find(&found, "src/billing/feature/area/ui.test.ts", "dependency");
    assert_eq!(test["from"], "tests");

    // A reason on the listed name beats the reason of the rule.
    assert_eq!(
        find(&found, "src/core/util.ts", "dependency")["because"],
        "core never knows enterprise code"
    );
    let externals: Vec<_> = found.iter().filter(|v| v["rule"] == "external").collect();
    assert_eq!(externals.len(), 2);
    let because =
        |target: &str| externals.iter().find(|v| v["target"] == target).unwrap()["because"].clone();
    assert_eq!(because("moment"), "moment is deprecated, use date-fns");
    assert_eq!(because("lodash/fp"), "core stays dependency-free");

    // Only the file outside every component and outside `unowned` is an orphan.
    let orphans: Vec<_> = found.iter().filter(|v| v["rule"] == "orphan").collect();
    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0]["file"], "src/stray.ts");
}

#[test]
fn isolation_can_be_suppressed() {
    let (_tmp, root) = fixture("structure");
    let file = root.join("src/pages/contacts/index.ts");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        format!("// easement:disable-next-line isolation -- shared list\n{text}"),
    )
    .unwrap();
    let found = reports(&root, |_| {});
    assert!(!found.iter().any(|v| v["rule"] == "isolation"), "{found:?}");
}

#[test]
fn component_and_scope_narrow_the_report() {
    let (_tmp, root) = fixture("structure");
    let components = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();

    let core = reports(&root, |o| o.components = components(&["core"]));
    assert_eq!(core.len(), 3, "{core:?}");
    assert!(core.iter().all(|v| v["file"] == "src/core/util.ts"));

    let pages = reports(&root, |o| o.paths = vec!["src/pages".into()]);
    let files: Vec<_> = pages.iter().map(|v| v["file"].as_str().unwrap()).collect();
    assert_eq!(
        files,
        ["src/pages/contacts/index.ts", "src/pages/segments/list.ts"]
    );

    // Both restrictions must hold.
    assert!(
        reports(&root, |o| {
            o.components = components(&["core"]);
            o.paths = vec!["src/pages".into()];
        })
        .is_empty()
    );

    // A scope narrows the violations, not what was read: the counters, like the
    // cycles, limits and evidence gaps, are those of the whole project.
    let stats = |names: &[&str]| {
        json(&check_with(&root, |o| o.components = components(names)))["stats"].clone()
    };
    let (whole, pages) = (stats(&[]), stats(&["pages"]));
    for counter in ["files", "internal_deps", "external_deps"] {
        assert_eq!(pages[counter], whole[counter], "{counter}");
    }
    // The narrowing is in `narrowed`, not repeated among the counters.
    assert!(pages.get("limited_to").is_none(), "{pages}");

    let refused = error(try_check_with(&root, |o| {
        o.components = components(&["ghost"])
    }));
    assert!(refused.contains("unknown component `ghost`"), "{refused}");
    let refused = error(try_check_with(&root, |o| {
        o.components = components(&["pgaes"])
    }));
    assert!(
        refused.contains("unknown component `pgaes`; did you mean `pages`?"),
        "{refused}"
    );
}

#[test]
fn scoped_runs_do_not_report_stale_entries_of_other_files() {
    let (_tmp, root) = fixture("structure");
    let recorded = commands::baseline(&config(&root), false, &mut cache()).unwrap();
    assert_eq!(recorded.outcome(), Outcome::Passed);
    // Fix a violation inside `pages`: its baseline entry is stale there, not in `core`.
    fs::write(
        root.join("src/pages/segments/list.ts"),
        "export const list = 1;\n",
    )
    .unwrap();
    let checked = check_with(&root, |o| o.components = vec!["core".into()]);
    assert_eq!(checked.outcome(), Outcome::Passed);
    assert!(text(&checked).contains("0 stale"), "{}", text(&checked));
    let checked = check_with(&root, |o| o.components = vec!["pages".into()]);
    assert_eq!(checked.outcome(), Outcome::Failed);
    assert!(text(&checked).contains("1 stale"), "{}", text(&checked));
}

#[test]
fn required_imports_modules_and_files_are_checked_per_directory() {
    let (_tmp, root) = fixture("required");
    let found = reports(&root, |_| {});
    let got: Vec<(String, String)> = found
        .iter()
        .map(|v| {
            assert_eq!(v["rule"], "required");
            (
                v["file"].as_str().unwrap().into(),
                v["target"].as_str().unwrap().into(),
            )
        })
        .collect();
    let has = |file: &str, target: &str| {
        assert!(
            got.contains(&(file.into(), target.into())),
            "{file} {target}: {got:?}"
        );
    };
    // `books` has everything; `users` imports shared from one file but misses the rest.
    has("src/api/users/resources/b.ts", "external goverter");
    has("src/api/users/resources/b.ts", "file README.md");
    has("src/api/orders/resources/d.ts", "component shared");
    has("src/api/orders/resources/d.ts", "external goverter");
    has("src/api/orders/resources/d.ts", "file README.md");
    assert_eq!(got.len(), 5, "{got:?}");
    let goverter = found
        .iter()
        .find(|v| v["target"] == "external goverter")
        .unwrap();
    assert_eq!(
        goverter["because"], "mappers are generated with goverter",
        "{found:?}"
    );

    // Adding what is missing clears the violations of that directory.
    let users = root.join("src/api/users/resources");
    fs::write(users.join("README.md"), "# users\n").unwrap();
    fs::write(
        users.join("b.ts"),
        "import goverter from \"goverter\";\nexport const b = goverter;\n",
    )
    .unwrap();
    let found = reports(&root, |_| {});
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found
            .iter()
            .all(|v| v["file"] == "src/api/orders/resources/d.ts")
    );
}

#[test]
fn forbidden_files_and_required_directives_are_reported() {
    let (_tmp, root) = fixture("files");
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains("src/ui/a.css:1: ui must not contain src/ui/a.css (forbid_file *.css)"),
        "{report}"
    );
    assert!(
        report.contains("because: styles live in the design system"),
        "{report}"
    );
    assert!(
        report.contains(
            "src/conv/bad/m.ts:1: src/conv/bad (conv) must contain a comment or text matching `goverter:converter`"
        ),
        "{report}"
    );
    assert!(!report.contains("src/conv/ok"), "{report}");
    assert!(report.contains("2 violation(s)"), "{report}");
}

#[test]
fn tests_auto_gives_test_files_their_own_allow_list() {
    let (_tmp, root) = fixture("testsauto");
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains("src/app/x.test.ts:3: tests -> db is not allowed (written as src/db/d.ts)"),
        "{report}"
    );
    assert!(
        report.contains("because: tests reach the database only through testhelper"),
        "{report}"
    );
    assert!(report.contains("1 violation(s)"), "{report}");
    let said = commands::explain(&config(&root), &root.join("src/app/x.test.ts"))
        .unwrap()
        .text()
        .unwrap();
    assert!(said.contains("component: tests"), "{said}");
}
