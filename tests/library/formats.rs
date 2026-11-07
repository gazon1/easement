//! The report of `check` in the formats a CI reads: GitHub annotations,
//! SARIF and JUnit.

use easement::commands::Outcome;

use crate::common::fixture;
use crate::run::{check, parse};

#[test]
fn the_report_reads_in_every_ci_format() {
    let (_tmp, root) = fixture("policy");
    let checked = check(&root);
    assert_eq!(checked.outcome(), Outcome::Failed);
    let report = checked.report();

    let github = report.github();
    assert!(
        github.contains(
            "::error file=src/web/page.ts,line=2,title=easement dependency::web -> data is forbidden"
        ),
        "{github}"
    );

    let sarif = parse(&report.sarif());
    assert_eq!(sarif["version"], "2.1.0");
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 6);
    assert!(results.iter().any(|r| {
        r["ruleId"] == "orphan"
            && r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"] == "src/stray.ts"
    }));

    let junit = report.junit();
    assert!(junit.contains(
        "<testsuite name=\"easement\" tests=\"6\" skipped=\"0\" errors=\"0\" failures=\"6\">"
    ));
    assert!(junit.contains("type=\"external\""));
}
