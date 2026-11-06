//! `check` through the library: what the verdict holds and what it leaves out,
//! on scratch projects indexed by the real indexers.

use std::fs;
use std::path::{Path, PathBuf};

use easement::commands::{self, CheckOptions, Checked, Outcome};
use easement::{Cache, CacheUse, GapKind, Jobs};
use tempfile::TempDir;

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

/// A scratch copy of the fixture project `name`, so a test can edit its code.
fn fixture(name: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    copy_dir(&src, tmp.path());
    let root = tmp.path().to_path_buf();
    (tmp, root)
}

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
    // An answer about the graph says it may be short.
    let why = commands::why(
        &root.join("easement.yml"),
        "a",
        "b",
        &mut Cache::new(CacheUse::Off, Jobs::default()),
    )
    .unwrap();
    assert!(
        why.coverage
            .evidence_gaps
            .iter()
            .any(|gap| gap.kind == GapKind::UnresolvedImports),
        "{:?}",
        why.coverage
    );
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
