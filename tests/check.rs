//! `check` through the library: what the verdict holds and what it leaves out,
//! on scratch projects indexed by the real indexers.

use std::fs;
use std::path::Path;

use easement::commands::{self, CheckOptions, Checked, Outcome, Proposed};
use easement::{Cache, CacheUse, Coverage, GapKind, Jobs};

mod common;

use common::fixture;

/// The check of the whole project at `root`, indexed afresh.
fn check(root: &Path, require_evidence: bool) -> Checked {
    let options = CheckOptions {
        require_evidence,
        ..CheckOptions::new(root.join("easement.yml"))
    };
    commands::check(&options, &mut Cache::new(CacheUse::Off, Jobs::default())).unwrap()
}

/// The message of each gap of `kind`.
fn gaps(checked: &Checked, kind: GapKind) -> Vec<&str> {
    checked
        .coverage
        .evidence_gaps
        .iter()
        .filter(|gap| gap.kind == kind)
        .map(|gap| gap.message.as_str())
        .collect()
}

/// scip-go resolves one side of an import cycle only: the import it dropped is
/// an evidence gap that names it, not a dependency that does not exist, and
/// not a clean pass on the forbidden `a -> b` it hides.
#[test]
fn an_import_the_indexer_did_not_resolve_is_an_evidence_gap() {
    let (_tmp, root) = fixture("go_import_cycle");
    let checked = check(&root, true);
    assert_eq!(checked.outcome(), Outcome::EvidenceRequired);
    let [gap] = checked.coverage.evidence_gaps.as_slice() else {
        panic!("one gap: {:?}", checked.coverage.evidence_gaps);
    };
    assert_eq!(gap.kind, GapKind::UnresolvedImports);
    assert_eq!(gap.files, ["a/a.go"]);
    assert!(
        gap.message.contains("a/a.go:3 (`example.com/repro/b`)"),
        "{}",
        gap.message
    );
}

/// An import the indexer dropped is a dependency missing from the graph, so
/// every answer about the graph says it may be short.
#[test]
fn answers_about_the_graph_say_an_unresolved_import_may_hide_a_dependency() {
    let (_tmp, root) = fixture("go_import_cycle");
    let config = root.join("easement.yml");
    let cache = || Cache::new(CacheUse::Off, Jobs::default());
    let says_so = |coverage: &Coverage| {
        coverage
            .evidence_gaps
            .iter()
            .any(|gap| gap.kind == GapKind::UnresolvedImports && gap.files == ["a/a.go"])
    };
    let why = commands::why(&config, "a", "b", &mut cache()).unwrap();
    assert!(says_so(&why.coverage), "why: {:?}", why.coverage);
    let impacted = commands::impact(&config, &[root.join("b/b.go")], None, &mut cache()).unwrap();
    assert!(
        says_so(&impacted.impact.coverage),
        "impact: {:?}",
        impacted.impact.coverage
    );
    let proposed = Proposed::One {
        source: root.join("b/b.go"),
        target: Some(root.join("a/a.go")),
        external: None,
    };
    let report = commands::propose(&config, &proposed, false, &mut cache()).unwrap();
    assert!(says_so(&report.coverage), "propose: {:?}", report.coverage);
}

/// A ban that a deeper name shows to be misspelled fails as `unmatched` only
/// over a whole index (ADR 0004): an import the indexer dropped leaves it short,
/// so the ban says nothing until the import is resolved.
#[test]
fn a_forbid_symbol_that_can_never_fire_waits_for_an_unresolved_import() {
    let (_tmp, root) = fixture("go_import_cycle");
    fs::write(
        root.join("a/t.go"),
        "package a\n\ntype T struct{}\n\nfunc (T) M() {}\n",
    )
    .unwrap();
    let config = root.join("easement.yml");
    let policy = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        format!("{policy}forbid_symbol: [\"example.com/repro/a.M\"]\n"),
    )
    .unwrap();
    let unmatched = |checked: &Checked| {
        checked
            .verdict
            .new
            .iter()
            .filter(|v| {
                v.message.contains("`example.com/repro/a.M`")
                    && v.message.contains("can never fire")
            })
            .count()
    };
    let checked = check(&root, false);
    assert!(!gaps(&checked, GapKind::UnresolvedImports).is_empty());
    assert_eq!(unmatched(&checked), 0, "{:?}", checked.verdict.new);

    // With the cycle gone, the index is whole and the ban can be judged.
    fs::write(root.join("a/a.go"), "package a\n").unwrap();
    let checked = check(&root, false);
    assert!(gaps(&checked, GapKind::UnresolvedImports).is_empty());
    assert_eq!(unmatched(&checked), 1, "{:?}", checked.verdict.new);
}

/// An import is resolved by a module on its own path, not by any on its line:
/// the dropped import of `b` beside a resolved `os` is still a gap.
#[test]
fn an_unresolved_import_beside_a_resolved_one_on_its_line_is_an_evidence_gap() {
    let (_tmp, root) = fixture("go_import_cycle");
    fs::write(
        root.join("a/a.go"),
        "package a\n\nimport ( \"os\"; _ \"example.com/repro/b\" )\n\nvar _ = os.Args\n",
    )
    .unwrap();
    let checked = check(&root, true);
    assert_eq!(checked.outcome(), Outcome::EvidenceRequired);
    assert_eq!(
        gaps(&checked, GapKind::UnresolvedImports),
        [
            "the indexer did not resolve 1 import(s), so the dependencies they write were not checked (for example, a package that does not type-check, or an import cycle): a/a.go:3 (`example.com/repro/b`)"
        ]
    );
}

/// Every way Go writes an import is resolved by the indexer, so none is a gap.
/// An import of a package the module does not have is a dependency on the
/// module it spells, which the rules judge: never silent.
#[test]
fn every_form_of_a_go_import_is_resolved_and_a_missing_package_is_judged() {
    let (_tmp, root) = fixture("go_imports");
    let checked = check(&root, true);
    assert!(
        checked.coverage.evidence_gaps.is_empty(),
        "{:?}",
        checked.coverage.evidence_gaps
    );
    assert_eq!(checked.outcome(), Outcome::Passed);

    // scip-go files the missing package under the standard library, but `$std`
    // is told by the path, which names a domain.
    fs::write(
        root.join("lib/missing.go"),
        "package lib\n\nimport \"example.com/forms/nowhere\"\n\nvar _ = nowhere.X\n",
    )
    .unwrap();
    let checked = check(&root, false);
    assert!(
        checked
            .verdict
            .new
            .iter()
            .any(|v| v.file == "lib/missing.go" && v.line == 3),
        "{:?}",
        checked.verdict.new
    );
    assert!(gaps(&checked, GapKind::UnresolvedImports).is_empty());
}

/// cgo's `import "C"` names no package, so it is never an unresolved import,
/// whether or not the indexer could read the file (that takes a C toolchain).
#[test]
fn cgo_is_no_unresolved_import() {
    let (_tmp, root) = fixture("go_imports");
    fs::write(
        root.join("app/cgo.go"),
        "package app\n\n// #include <stdlib.h>\nimport \"C\"\n\nimport \"os\"\n\nvar _ = os.Args\n",
    )
    .unwrap();
    let checked = check(&root, false);
    assert!(
        checked
            .coverage
            .evidence_gaps
            .iter()
            .all(|gap| gap.kind != GapKind::UnresolvedImports),
        "{:?}",
        checked.coverage.evidence_gaps
    );
}
