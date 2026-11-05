//! Policy examples: proposed dependencies the policy keeps with the decision
//! each must get. Every `check` judges them as `propose` judges a proposal,
//! through [`crate::decision::decide`], so a change to the policy that flips
//! one fails as `example` at its line although no code changed.

use std::path::Path;

use crate::check::{RuleId, Violation};
use crate::components::ComponentMap;
use crate::config::{Example, Expect};
use crate::policy::Policy;
use crate::propose::{self, Judged, Kind, Proposal, To};

/// One violation for each example of the policy that does not get the
/// decision it expects, at its line in the policy input that writes it.
pub fn check(policy: &Policy, map: &ComponentMap, root: &Path) -> Vec<Violation> {
    let examples = &policy.config.examples;
    let proposals: Vec<Proposal> = examples.iter().map(proposal).collect();
    let judged = propose::judge(&policy.config, map, &proposals);
    examples
        .iter()
        .zip(&proposals)
        .zip(&judged)
        .enumerate()
        .filter_map(|(i, ((example, proposal), judged))| {
            let problem = problem(example, judged)?;
            let (file, line) = policy.example_at(i).map_or_else(
                || (String::new(), 1),
                |(input, line)| (crate::diff::input_path(input, root), line),
            );
            Some(Violation {
                rule: RuleId::Example,
                file,
                line,
                message: format!("example {}: {problem}", proposal.label()),
                because: example.because.clone(),
                from: judged.from.clone(),
                to: judged.target(),
                target: expected(example),
                specifier: String::new(),
                evidence: Vec::new(),
                chain: None,
                provenance: None,
            })
        })
        .collect()
}

fn proposal(example: &Example) -> Proposal {
    Proposal {
        from: example.from.clone(),
        to: match (&example.to, &example.external) {
            (Some(path), _) => To::Path(path.clone()),
            (None, module) => To::External(module.clone().unwrap_or_default()),
        },
    }
}

/// What the example expects, as a reader says it: `rejected [privacy]`.
fn expected(example: &Example) -> String {
    match example.rule {
        Some(rule) => format!("{} [{}]", example.expect.as_str(), rule.id().as_str()),
        None => example.expect.as_str().to_string(),
    }
}

/// What is wrong with the decision on the example, if anything.
fn problem(example: &Example, judged: &Judged) -> Option<String> {
    if let Some(why) = unjudged(judged) {
        return Some(format!(
            "cannot be judged: {why}; point it at a path a component owns, or drop it"
        ));
    }
    let got = match (judged.decision, judged.rule) {
        (Kind::Rejected, Some(rule)) => Some(rule),
        _ => None,
    };
    let holds = match (example.expect, got) {
        (Expect::Allowed, None) => true,
        (Expect::Rejected, Some(rule)) => example.rule.is_none_or(|r| r.id() == rule),
        _ => false,
    };
    if holds {
        return None;
    }
    let decision = match (got, &judged.message) {
        (Some(_), Some(message)) => format!("{}: {message}", judged.verdict()),
        _ => judged.verdict(),
    };
    Some(format!(
        "expects {}, but the policy now decides {decision}; restore what the policy meant, or update the example if the change is meant",
        expected(example)
    ))
}

/// Why no decision on the example says anything of the policy: a source or a
/// target path that no component owns, `exclude` hiding it among the reasons.
fn unjudged(judged: &Judged) -> Option<String> {
    match judged.decision {
        Kind::Excluded => return Some(format!("`exclude` hides {}", judged.from)),
        Kind::TargetOnly => return Some(format!("`target_only` names {}", judged.from)),
        Kind::Unowned => return Some(format!("{} belongs to no component", judged.from)),
        Kind::Allowed | Kind::Rejected => {}
    }
    let to = judged.to.as_ref()?;
    if judged.target_component.is_some() {
        return None;
    }
    Some(if judged.rule == Some(RuleId::Excluded) {
        format!("`exclude` hides {to}")
    } else {
        format!("{to} belongs to no component")
    })
}
