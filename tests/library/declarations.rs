//! What an import that more than one file declares says about itself: the
//! evidence gap it leaves, the resolution a violation carries, and the
//! candidates a dependency lists.

use std::path::PathBuf;

use easement::commands;
use tempfile::TempDir;

use crate::common::{fixture, project};
use crate::run::{cache, check, json, leaves_out, pinned};

/// The file that imports the name.
const IMPORT: &str = "app/src/main/kotlin/com/acme/App.kt";

/// A Kotlin project where `app` imports `com.acme.thing.Thing`, and each pair
/// of `copies` is the component that holds a copy of it and the file that
/// declares it.
fn project_of(copies: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let mut components = vec!["app"];
    for (component, _) in copies {
        if !components.contains(component) {
            components.push(component);
        }
    }
    let policy = format!(
        "version: 1\n\ncomponents:\n{}\nrules:\n  app: {{ must_not_depend_on: [lib] }}\n",
        components
            .iter()
            .map(|component| format!("  {component}: {{ in: {component} }}\n"))
            .collect::<String>()
    );
    let mut files: Vec<(String, String)> = vec![
        ("easement.yml".into(), policy),
        (
            IMPORT.into(),
            "package com.acme\n\nimport com.acme.thing.Thing\n\nclass App {\n    val thing = Thing()\n}\n"
                .into(),
        ),
    ];
    for (_, path) in copies {
        files.push((
            (*path).into(),
            "package com.acme.thing\n\nclass Thing\n".into(),
        ));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    project(&files)
}

/// Two files that declare one name leave a gap: the import of it may mean
/// either, so the graph may be short.
#[test]
fn a_name_declared_in_two_components_is_an_evidence_gap() {
    let (_tmp, root) = fixture("kotlin_duplicate");
    let report = json(&check(&root));
    let gaps: Vec<_> = report["evidence_gaps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|gap| gap["kind"] == "declarations")
        .collect();
    assert_eq!(gaps.len(), 1, "{report}");
    assert!(leaves_out(&report), "{report}");
    pinned(|| {
        insta::assert_json_snapshot!("declarations_gap", gaps[0]);
    });
}

/// A dependency that two components may answer for says how certain its
/// target is: the target is not proven.
#[test]
fn a_violation_over_two_components_says_the_target_is_ambiguous() {
    let (_tmp, root) = project_of(&[
        ("lib", "lib/src/main/kotlin/com/acme/thing/Thing.kt"),
        ("other", "other/src/main/kotlin/com/acme/thing/Thing.kt"),
    ]);
    let report = json(&check(&root));
    let violation = &report["violations"][0];
    assert_eq!(violation["rule"], "dependency");
    assert_eq!(violation["from"], "app");
    assert_eq!(violation["to"], "lib");
    assert_eq!(violation["resolution"], "Ambiguous");
    // The copy of the component of the importing file, or the first by path.
    assert_eq!(
        violation["target"],
        "lib/src/main/kotlin/com/acme/thing/Thing.kt"
    );
}

/// Two declarations in one component leave the component known and the exact
/// file unknown.
#[test]
fn two_declarations_in_one_component_are_the_same_scope() {
    let (_tmp, root) = project_of(&[
        ("lib", "lib/src/main/kotlin/com/acme/thing/Thing.kt"),
        ("lib", "lib/src/main/kotlin/com/acme/other/Thing.kt"),
    ]);
    let report = json(&check(&root));
    let violation = &report["violations"][0];
    assert_eq!(violation["to"], "lib");
    assert_eq!(violation["resolution"], "SameScope");
}

/// A target that one file declares is proven, so the violation says nothing
/// of how certain it is.
#[test]
fn a_proven_target_carries_no_resolution() {
    let (_tmp, root) = project_of(&[("lib", "lib/src/main/kotlin/com/acme/thing/Thing.kt")]);
    let report = json(&check(&root));
    let violation = &report["violations"][0];
    assert_eq!(violation["rule"], "dependency");
    assert!(violation.get("resolution").is_none(), "{violation}");
    assert!(
        report["evidence_gaps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|gap| gap["kind"] != "declarations"),
        "{report}"
    );
}

/// The dependency lists every file that may declare the name, so a reader
/// knows what else the import may mean.
#[test]
fn a_dependency_lists_the_declarations_the_import_may_mean() {
    let (_tmp, root) = project_of(&[
        ("lib", "lib/src/main/kotlin/com/acme/thing/Thing.kt"),
        ("other", "other/src/main/kotlin/com/acme/thing/Thing.kt"),
    ]);
    let why = commands::why(&root.join("easement.yml"), "app", "lib", &mut cache()).unwrap();
    let why = serde_json::to_value(&why).unwrap();
    let decision = &why["dependencies"][0]["decisions"][0];
    assert_eq!(decision["resolution"], "Ambiguous");
    assert_eq!(
        decision["candidates"],
        serde_json::json!([
            "lib/src/main/kotlin/com/acme/thing/Thing.kt",
            "other/src/main/kotlin/com/acme/thing/Thing.kt"
        ])
    );
}
