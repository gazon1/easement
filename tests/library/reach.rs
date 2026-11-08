//! `must_not_reach`: a component that reaches another through a chain of
//! imports, each hop of it allowed, with the chain as evidence; and named
//! placeholders, which keep the dependencies of a slice within it.

use std::fs;
use std::path::PathBuf;

use easement::commands::{self, Outcome};
use tempfile::TempDir;

use crate::common::{commit_all, fixture, project};
use crate::run::{cache, check, check_with, config, json, text};

/// `domain` must not reach `http`, even through `util`, though every hop on
/// its own is allowed.
pub fn reach_project(extra: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let mut files = vec![
        (
            "easement.yml",
            "version: 1\ncomponents:\n  domain: { in: src/domain }\n  util: { in: src/util }\n  http: { in: src/http }\nrules:\n  domain:\n    may_depend_on: [util]\n    must_not_reach: [{ name: http, because: the domain knows no transport }]\n  util: { may_depend_on: [http] }\n",
        ),
        (
            "src/domain/Order.kt",
            "package domain\nimport util.Retry\nclass Order\n",
        ),
        (
            "src/util/Retry.kt",
            "package util\nimport http.Client\nclass Retry\n",
        ),
        ("src/http/Client.kt", "package http\nclass Client\n"),
    ];
    files.extend_from_slice(extra);
    project(&files)
}

/// Go compiles a package whole: an import names the file that defines what it uses,
/// but the other files of the package come along, and so do their imports. Test
/// files do not: they are never linked into an importer.
pub fn go_package_reach(top: &str) -> (TempDir, PathBuf) {
    project(&[
        ("go.mod", "module example.com/app\n\ngo 1.22\n"),
        (
            "easement.yml",
            &format!(
                "version: 1\n{top}components:\n  domain: {{ in: internal/domain }}\n  util: {{ in: internal/util }}\n  http: {{ in: internal/http }}\n  mock: {{ in: internal/mock }}\nrules:\n  domain: {{ may_depend_on: [util], must_not_reach: [http, mock] }}\n  util: {{ may_depend_on: [http, mock] }}\n"
            ),
        ),
        (
            "internal/domain/order.go",
            "package domain\n\nimport \"example.com/app/internal/util\"\n\nfunc Total() int { return util.Add(1, 2) }\n",
        ),
        (
            "internal/util/math.go",
            "package util\n\nfunc Add(a, b int) int { return a + b }\n",
        ),
        (
            "internal/util/retry.go",
            "package util\n\nimport \"example.com/app/internal/http\"\n\nfunc Retry() { http.Send() }\n",
        ),
        (
            "internal/util/retry_test.go",
            "package util\n\nimport \"example.com/app/internal/mock\"\n\nvar _ = mock.Fake\n",
        ),
        (
            "internal/http/client.go",
            "package http\n\nfunc Send() {}\n",
        ),
        ("internal/mock/fake.go", "package mock\n\nvar Fake = 1\n"),
    ])
}

/// Vertical slices: a feature's UI may use its own data and any feature's API.
pub fn slices_project(extra: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let mut files = vec![
        (
            "easement.yml",
            "version: 1\ncomponents:\n  ui: { in: 'src/features/<feature>/ui' }\n  data: { in: 'src/features/<feature>/data' }\n  api: { in: 'src/features/*/api' }\n  app: { in: src/app }\nrules:\n  ui: { may_depend_on: [data, api] }\n  app: { may_depend_on: [ui] }\n",
        ),
        (
            "src/features/orders/ui/View.kt",
            "package features.orders.ui\nimport features.orders.data.Repo\nimport features.billing.api.Api\nclass View\n",
        ),
        (
            "src/features/orders/data/Repo.kt",
            "package features.orders.data\nclass Repo\n",
        ),
        (
            "src/features/billing/api/Api.kt",
            "package features.billing.api\nclass Api\n",
        ),
        (
            "src/features/billing/data/Ledger.kt",
            "package features.billing.data\nclass Ledger\n",
        ),
        (
            "src/app/Main.kt",
            "package app\nimport features.orders.ui.View\nclass Main\n",
        ),
    ];
    files.extend_from_slice(extra);
    project(&files)
}

#[test]
fn a_component_must_not_reach_another_through_a_chain() {
    let (_tmp, root) = reach_project(&[]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains(
            "src/domain/Order.kt:2: domain reaches http through util: src/domain/Order.kt:2 -> src/util/Retry.kt:2 -> src/http/Client.kt [reach]",
        ),
        "{report}"
    );
    assert!(
        report.contains("because: the domain knows no transport"),
        "{report}"
    );
}

#[test]
fn reach_follows_files_of_no_component_and_leaves_direct_imports_to_dependency() {
    // Through a file that belongs to no component.
    let (_tmp, root) = reach_project(&[
        (
            "src/domain/Order.kt",
            "package domain\nimport misc.Glue\nclass Order\n",
        ),
        (
            "src/misc/Glue.kt",
            "package misc\nimport http.Client\nclass Glue\n",
        ),
    ]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains(
            "domain reaches http through no component: src/domain/Order.kt:2 -> src/misc/Glue.kt:2 -> src/http/Client.kt [reach]",
        ),
        "{report}"
    );
    // Through an import of a whole package, which resolves to its directory.
    let (_tmp, root) = reach_project(&[(
        "src/domain/Order.kt",
        "package domain\nimport util.*\nclass Order\n",
    )]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains(
            "domain reaches http through util: src/domain/Order.kt:2 -> src/util/Retry.kt:2 -> src/http/Client.kt [reach]",
        ),
        "{report}"
    );
    // Straight into it: forbidden, and not a chain.
    let (_tmp, root) = reach_project(&[(
        "src/domain/Order.kt",
        "package domain\nimport http.Client\nclass Order\n",
    )]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(report.contains("domain -> http is forbidden"), "{report}");
    assert!(!report.contains("[reach]"), "{report}");
}

#[test]
fn a_baselined_reach_stays_legal_and_diff_reports_a_new_one() {
    let (_tmp, root) = reach_project(&[]);
    let recorded = commands::baseline(&config(&root), false, &mut cache()).unwrap();
    assert_eq!(recorded.outcome(), Outcome::Passed);
    // Another file on the same chain is not new debt.
    fs::write(
        root.join("src/domain/Account.kt"),
        "package domain\nimport util.Retry\nclass Account\n",
    )
    .unwrap();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Passed);
    assert!(text(&checked).contains("1 legalized"), "{}", text(&checked));

    // A pull request that adds the last hop is judged in diff mode.
    let (_tmp, root) = reach_project(&[("src/util/Retry.kt", "package util\nclass Retry\n")]);
    commit_all(&root);
    fs::write(
        root.join("src/util/Retry.kt"),
        "package util\nimport http.Client\nclass Retry\n",
    )
    .unwrap();
    let checked = check_with(&root, |o| o.diff = Some("main".into()));
    assert_eq!(checked.outcome(), Outcome::Failed);
    assert!(
        text(&checked).contains("domain reaches http through util"),
        "{}",
        text(&checked)
    );
}

#[test]
fn reach_crosses_go_packages() {
    let (_tmp, root) = project(&[
        ("go.mod", "module example.com/app\n\ngo 1.22\n"),
        (
            "easement.yml",
            "version: 1\ncomponents:\n  domain: { in: internal/domain }\n  util: { in: internal/util }\n  http: { in: internal/http }\nrules:\n  domain: { may_depend_on: [util], must_not_reach: [http] }\n  util: { may_depend_on: [http] }\n",
        ),
        (
            "internal/domain/order.go",
            "package domain\n\nimport \"example.com/app/internal/util\"\n\nvar Order = util.Retry\n",
        ),
        (
            "internal/util/retry.go",
            "package util\n\nimport \"example.com/app/internal/http\"\n\nvar Retry = http.Client\n",
        ),
        (
            "internal/http/client.go",
            "package http\n\nvar Client = 1\n",
        ),
    ]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    assert!(
        text(&checked).contains(
            "domain reaches http through util: internal/domain/order.go:3 -> internal/util/retry.go:3 -> internal/http/client.go [reach]",
        ),
        "{}",
        text(&checked)
    );
}

#[test]
fn reach_through_another_file_of_a_go_package() {
    for top in ["", "reach: symbols\n"] {
        let (_tmp, root) = go_package_reach(top);
        let checked = check(&root);
        assert_eq!(checked.outcome(), Outcome::Failed, "{top}");
        let out = text(&checked);
        assert!(
            out.contains("domain reaches http through util: internal/domain/order.go:")
                && out.contains("-> internal/util/retry.go:3 -> internal/http/client.go [reach]"),
            "{top}{out}"
        );
        assert!(!out.contains("reaches mock"), "{top}{out}");
        assert!(out.contains("1 violation(s)"), "{top}{out}");
    }
}

#[test]
fn a_named_placeholder_keeps_dependencies_within_one_slice() {
    let (_tmp, root) = slices_project(&[]);
    assert_eq!(check(&root).outcome(), Outcome::Passed);

    let (_tmp, root) = slices_project(&[(
        "src/features/billing/ui/Pay.kt",
        "package features.billing.ui\nimport features.orders.data.Repo\nimport features.orders.ui.View\nclass Pay\n",
    )]);
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = text(&checked);
    assert!(
        report.contains(
            "src/features/billing/ui/Pay.kt:2: ui -> data: <feature> differs (billing depends on orders: src/features/orders/data/Repo.kt) [isolation]",
        ),
        "{report}"
    );
    assert!(
        report.contains(
            "src/features/billing/ui/Pay.kt:3: ui -> ui: <feature> differs (billing depends on orders: src/features/orders/ui/View.kt) [isolation]",
        ),
        "{report}"
    );
}

/// From the JSON alone an agent names the rule, every hop of the chain and
/// the policy input that prohibits it: here a file the configuration extends.
#[test]
fn a_reach_violation_carries_its_chain_evidence_and_the_input_that_prohibits_it() {
    let (_tmp, root) = fixture("reach");
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = json(&checked);
    let reach = &report["violations"][0];
    assert_eq!(reach["rule"], "reach", "{report}");
    assert_eq!(
        reach["chain"],
        serde_json::json!({
            "witness": "shortest",
            "hops": [
                { "file": "src/domain/Order.kt", "line": 2, "from_component": "domain", "to_component": "shared" },
                { "file": "src/shared/Retry.kt", "line": 2, "from_component": "shared", "to_component": "http" },
            ],
        }),
        "{report}"
    );
    assert_eq!(
        reach["evidence"],
        serde_json::json!([
            { "file": "src/domain/Order.kt", "line": 2 },
            { "file": "src/shared/Retry.kt", "line": 2 },
        ]),
        "{report}"
    );
    assert_eq!(
        reach["provenance"],
        serde_json::json!({ "field": "rules.domain.must_not_reach[0]", "input": "policy/base.yml" }),
        "{report}"
    );

    // A path outside the surface of its component: the clause is the surface.
    fs::create_dir_all(root.join("src/http/pool")).unwrap();
    fs::write(
        root.join("src/http/pool/Pool.kt"),
        "package http.pool\nclass Pool\n",
    )
    .unwrap();
    fs::write(
        root.join("src/shared/Leak.kt"),
        "package shared\nimport http.pool.Pool\nclass Leak\n",
    )
    .unwrap();
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = json(&checked);
    let privacy = report["violations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["rule"] == "privacy")
        .unwrap_or_else(|| panic!("{report}"));
    assert_eq!(
        privacy["provenance"],
        serde_json::json!({ "field": "components.http.public", "input": "easement.yml" }),
        "{report}"
    );
}
