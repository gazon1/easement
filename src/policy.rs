//! The policy as loaded: the config merged from the files it extends, every
//! policy input with its content, and the input that defined each component
//! and rule. It is read from the working tree, or as it was at a Git revision.

use std::collections::BTreeMap;
use std::path::{Component as PathPart, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_norway::{Mapping, Value};

use crate::config::Config;

/// What a policy input is to the policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// The configuration the policy is loaded from.
    Config,
    /// A configuration that `extends` names, directly or through another one.
    Extended,
    /// The diagram that `diagram` names.
    Diagram,
}

/// A file whose content is part of the policy, as it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyInput {
    pub kind: InputKind,
    /// Absolute, where the file is in the working tree.
    pub path: PathBuf,
    pub content: String,
    /// Read from disk although a revision was asked, because the file is
    /// outside the repository: its content at that revision is unknown.
    pub past_unknown: bool,
}

/// The policy and the inputs it was loaded from.
#[derive(Debug)]
pub struct Policy {
    pub config: Config,
    /// The configuration first, then every file it extends in the order they
    /// are read, then the diagram if it could be read.
    pub inputs: Vec<PolicyInput>,
    /// The input that last defined an entry, by (`components` or `rules`, name).
    origins: Origins,
    /// Where each of `config.examples` is written.
    examples: Vec<ExampleOrigin>,
    /// Why the diagram the config names could not be read; raised only by the
    /// rule that needs it, so other commands work without it.
    diagram_error: Option<String>,
}

/// Where a policy example is written: the index of its input in
/// `Policy::inputs`, and its own index among the examples of that input.
#[derive(Debug, Clone, Copy)]
struct ExampleOrigin {
    input: usize,
    item: usize,
}

/// The input that last defined an entry, by (key, name): `components` and
/// `rules` by entry name, and the top-level keys under an empty key.
type Origins = BTreeMap<(String, String), usize>;

impl Policy {
    /// Loads the policy from the configuration at `path`, as it is in the
    /// working tree or, with `revision`, as it was at that Git revision.
    pub fn load(path: &Path, revision: Option<&str>) -> Result<Self> {
        let mut loader = Loader {
            reader: Reader::new(path, revision)?,
            stack: Vec::new(),
            inputs: Vec::new(),
            origins: Origins::new(),
            examples: Vec::new(),
        };
        let value = loader.load(path, InputKind::Config)?;
        let Loader {
            reader,
            mut inputs,
            mut origins,
            examples,
            ..
        } = loader;
        let config = Config::from_merged(Value::Mapping(value), path)?;
        if config.tests.is_some()
            && let Some(&at) = origins.get(&(String::new(), "tests".to_string()))
        {
            // `tests: auto` makes the component.
            origins.insert(
                ("components".to_string(), crate::config::TESTS.to_string()),
                at,
            );
        }
        let mut diagram_error = None;
        if let Some(diagram) = &config.diagram {
            // Relative to the root, the directory of the configuration.
            let root = inputs[0].path.parent().unwrap_or(Path::new("/"));
            let path = real(&normalize(&root.join(diagram)));
            match reader.read(&path) {
                Ok((content, past_unknown)) => inputs.push(PolicyInput {
                    kind: InputKind::Diagram,
                    path,
                    content,
                    past_unknown,
                }),
                Err(err) => {
                    diagram_error = Some(format!(
                        "{:#}",
                        err.context(format!("cannot read the diagram {}", diagram.display()))
                    ));
                }
            }
        }
        Ok(Self {
            config,
            inputs,
            origins,
            examples,
            diagram_error,
        })
    }

    /// The configuration the policy is loaded from.
    pub fn config_input(&self) -> &PolicyInput {
        &self.inputs[0]
    }

    /// The diagram the config names, if it names one; an error when it cannot
    /// be read.
    pub fn diagram(&self) -> Result<Option<&PolicyInput>> {
        if let Some(error) = &self.diagram_error {
            return Err(anyhow!("{error}"));
        }
        Ok(self.inputs.iter().find(|i| i.kind == InputKind::Diagram))
    }

    /// The input that writes the `index`th of `config.examples`, and the line,
    /// counted from 1, of its item there: the line of `examples:` when the
    /// list is not a block of `- ` items, such as a flow sequence.
    pub fn example_at(&self, index: usize) -> Option<(&PolicyInput, usize)> {
        let origin = self.examples.get(index)?;
        let input = self.inputs.get(origin.input)?;
        Some((input, item_line(&input.content, "examples", origin.item)))
    }

    /// The input that last defined `name` under `key`, `components` or
    /// `rules`: an extending file replaces a whole entry of that name. Under
    /// `common`, the input that first listed `name`; under an empty key, the
    /// last that wrote the top-level key `name`.
    pub fn defined_by(&self, key: &str, name: &str) -> Option<&PolicyInput> {
        let at = self.origins.get(&(key.to_string(), name.to_string()))?;
        self.inputs.get(*at)
    }
}

/// Where the policy writes a clause: its normalized field path
/// (`rules.web.must_not_depend_on[1]`), and the entry whose defining input
/// holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub field: String,
    /// `components` or `rules` and the name of the entry, `common` and the
    /// name it lists, or an empty key and a top-level key.
    entry: (&'static str, String),
}

impl Site {
    /// A clause of the entry `name` under `section`, `components` or `rules`.
    pub fn of_entry(section: &'static str, name: &str, field: String) -> Self {
        Self {
            field,
            entry: (section, name.to_string()),
        }
    }

    /// The top-level `key`, a clause in itself.
    pub fn of_key(key: &str) -> Self {
        Self {
            field: key.to_string(),
            entry: ("", key.to_string()),
        }
    }

    /// The item `name` of `common`, at `index` of the merged list.
    pub fn of_common(name: &str, index: usize) -> Self {
        Self {
            field: format!("common[{index}]"),
            entry: ("common", name.to_string()),
        }
    }
}

/// Where the policy writes a clause, as reports give it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    /// The normalized field path in the merged policy.
    pub field: String,
    /// The policy input that defined it, relative to the root of the policy,
    /// or absolute when it is outside the root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
}

impl Policy {
    /// Where `site` is written: its field path and the input that defined it.
    pub fn provenance(&self, site: &Site) -> Provenance {
        let (section, name) = &site.entry;
        Provenance {
            field: site.field.clone(),
            input: self.defined_by(section, name).map(|i| self.input_name(i)),
        }
    }

    /// How reports name `input`: relative to the root of the policy, the
    /// directory of the configuration, or absolute outside it.
    pub fn input_name(&self, input: &PolicyInput) -> String {
        let root = self.inputs[0].path.parent().unwrap_or(Path::new("/"));
        crate::relpath::slash(input.path.strip_prefix(root).unwrap_or(&input.path))
    }
}

/// Where the inputs are read from: the working tree, or a Git revision.
struct Reader {
    revision: Option<Revision>,
}

struct Revision {
    rev: String,
    /// The top of the repository, as a real path.
    top: PathBuf,
}

impl Reader {
    fn new(config: &Path, revision: Option<&str>) -> Result<Self> {
        let Some(rev) = revision else {
            return Ok(Self { revision: None });
        };
        let dir = real(&absolute(config)?)
            .parent()
            .map(Path::to_path_buf)
            .context("config has no parent directory")?;
        let top = crate::diff::git(&dir, &["rev-parse", "--show-toplevel"]).with_context(|| {
            format!("cannot read the policy at `{rev}`: not in a Git repository")
        })?;
        Ok(Self {
            revision: Some(Revision {
                rev: rev.to_string(),
                top: real(Path::new(top.trim())),
            }),
        })
    }

    /// The path that tells two inputs apart, so that a loop of `extends` is
    /// found: the real path in the working tree, the normalized one at a
    /// revision, where the file may no longer exist.
    fn identify(&self, path: &Path) -> Result<PathBuf> {
        match &self.revision {
            None => dunce::canonicalize(path)
                .with_context(|| format!("cannot read config {}", path.display())),
            Some(_) => Ok(real(&absolute(path)?)),
        }
    }

    /// The content of the file at `path` (absolute and normalized), and
    /// whether it was read from disk although its past was asked.
    fn read(&self, path: &Path) -> Result<(String, bool)> {
        let Some(Revision { rev, top }) = &self.revision else {
            return Ok((std::fs::read_to_string(path)?, false));
        };
        let Ok(inside) = path.strip_prefix(top) else {
            return Ok((std::fs::read_to_string(path)?, true));
        };
        let inside = crate::relpath::slash(inside);
        Ok((
            crate::diff::git(top, &["show", &format!("{rev}:{inside}")])?,
            false,
        ))
    }
}

/// Reads the configurations of a policy, keeping each file it reads and the
/// input that last defined each entry.
struct Loader {
    reader: Reader,
    /// The files being read, the outermost first: one read again extends itself.
    stack: Vec<PathBuf>,
    inputs: Vec<PolicyInput>,
    origins: Origins,
    /// The origin of each example, in the order the merge joins them.
    examples: Vec<ExampleOrigin>,
}

impl Loader {
    /// Reads a config file and merges the files it `extends` underneath it.
    ///
    /// `extends` is a path or a list of paths, relative to the file that names them.
    /// Patterns in every file stay relative to the root of the config being loaded.
    /// Keys that start with `x-` are dropped, so they can hold anchors for `<<`.
    /// `components` and `rules` merge by name, the later definition replacing the
    /// earlier one; `common`, `exclude` and `target_only` are concatenated, and so is
    /// `examples`, the bases' first and without dropping repeats; any other key is
    /// replaced by the extending file.
    fn load(&mut self, path: &Path, kind: InputKind) -> Result<Mapping> {
        let identity = self.reader.identify(path)?;
        if self.stack.contains(&identity) {
            bail!("config {} extends itself", path.display());
        }
        let (text, past_unknown) = self
            .reader
            .read(&identity)
            .with_context(|| format!("cannot read config {}", path.display()))?;
        let mut value = serde_norway::from_str::<Value>(&text)
            .with_context(|| format!("invalid config {}", path.display()))?;
        // `<<: *anchor` shares a part of a rule between components.
        value
            .apply_merge()
            .with_context(|| format!("invalid config {}", path.display()))?;
        let Value::Mapping(mut own) = value else {
            bail!("config {} is not a mapping", path.display());
        };
        let at = self.inputs.len();
        self.inputs.push(PolicyInput {
            kind,
            path: identity.clone(),
            content: text,
            past_unknown,
        });
        // `x-` keys are only places to define anchors.
        own.retain(|key, _| !key.as_str().is_some_and(|k| k.starts_with("x-")));
        let bases = match own.remove("extends") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(one)) => vec![one],
            Some(other) => serde_norway::from_value::<Vec<String>>(other).with_context(|| {
                format!("`extends` in {} must be a path or a list", path.display())
            })?,
        };
        // At a revision `..` is resolved by name, so it starts from the real
        // directory, as the working tree resolves it after the links.
        let dir = match self.reader.revision {
            None => path.parent(),
            Some(_) => identity.parent(),
        }
        .unwrap_or(Path::new("."))
        .to_path_buf();
        self.stack.push(identity);
        let mut merged = Mapping::new();
        for base in bases {
            let base = self.load(&dir.join(base), InputKind::Extended)?;
            merge(&mut merged, base);
        }
        self.stack.pop();
        // The bases pushed theirs while they were read: these follow them, as
        // `merge` appends them.
        if let Some(Value::Sequence(items)) = own.get("examples") {
            self.examples
                .extend((0..items.len()).map(|item| ExampleOrigin { input: at, item }));
        }
        for (key, value) in &own {
            let key = key.as_str().unwrap_or_default();
            self.origins.insert((String::new(), key.to_string()), at);
            match (key, value) {
                ("components" | "rules", Value::Mapping(entries)) => {
                    for name in entries.keys().filter_map(Value::as_str) {
                        self.origins.insert((key.to_string(), name.to_string()), at);
                    }
                }
                // Merged lists keep an item where it was first written.
                ("common", Value::Sequence(items)) => {
                    for name in items.iter().filter_map(Value::as_str) {
                        self.origins
                            .entry((key.to_string(), name.to_string()))
                            .or_insert(at);
                    }
                }
                _ => {}
            }
        }
        merge(&mut merged, own);
        Ok(merged)
    }
}

fn merge(into: &mut Mapping, from: Mapping) {
    for (key, value) in from {
        let name = key.as_str().unwrap_or_default();
        match (into.get_mut(&key), value) {
            (Some(Value::Mapping(old)), Value::Mapping(new))
                if name == "components" || name == "rules" =>
            {
                for (k, v) in new {
                    old.insert(k, v);
                }
            }
            (Some(Value::Sequence(old)), Value::Sequence(new))
                if matches!(name, "common" | "exclude" | "target_only") =>
            {
                for v in new {
                    if !old.contains(&v) {
                        old.push(v);
                    }
                }
            }
            (Some(Value::Sequence(old)), Value::Sequence(new)) if name == "examples" => {
                old.extend(new);
            }
            (_, value) => {
                into.insert(key, value);
            }
        }
    }
}

/// The line, counted from 1, of the `index`th `- ` item of the block sequence
/// under the top-level `key` in `text`; the line of `key` when there is no
/// such item, and 1 without `key`.
fn item_line(text: &str, key: &str, index: usize) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    let head = format!("{key}:");
    let Some(at) = lines.iter().position(|l| {
        l.strip_prefix(&head)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '#']))
    }) else {
        return 1;
    };
    let mut indent = None;
    let mut seen = 0;
    for (n, line) in lines.iter().enumerate().skip(at + 1) {
        let body = line.trim_start();
        if body.is_empty() || body.starts_with('#') {
            continue;
        }
        let depth = line.len() - body.len();
        let item = body == "-" || body.starts_with("- ");
        if depth == 0 && !item {
            break;
        }
        if item && *indent.get_or_insert(depth) == depth {
            if seen == index {
                return n + 1;
            }
            seen += 1;
        }
    }
    at + 1
}

fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(normalize(&std::env::current_dir()?.join(path)))
}

/// `path` with `.` and `..` resolved by name, not on disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            PathPart::CurDir => {}
            PathPart::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` (absolute and normalized) through the real path of the deepest
/// directory of it that exists, so that it compares with what Git reports.
fn real(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    while let Some(parent) = existing.parent() {
        if let Ok(real) = dunce::canonicalize(existing) {
            return rest.iter().rev().fold(real, |p, part| p.join(part));
        }
        rest.extend(existing.file_name());
        existing = parent;
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn git(dir: &Path, args: &[&str]) {
        crate::diff::git(dir, args).unwrap();
    }

    fn commit(dir: &Path, message: &str) {
        git(dir, &["add", "-A"]);
        git(
            dir,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                message,
            ],
        );
    }

    /// `extends` joins the examples, the bases' first, and each keeps the
    /// file and line that write it, whether a block item spans lines or the
    /// list is a flow sequence.
    #[test]
    fn examples_join_through_extends_and_keep_where_they_are_written() {
        let dir = tempfile::tempdir().unwrap();
        let example = |to: &str| format!("{{ from: a/x, to: {to}, expect: allowed }}");
        write(
            dir.path(),
            "deep.yml",
            &format!(
                "version: 1\nexamples: [{}, {}]\n",
                example("d1"),
                example("d2")
            ),
        );
        write(
            dir.path(),
            "base.yml",
            &format!(
                "extends: deep.yml\ncomponents:\n  a: {{ in: a }}\n# examples: [not, these]\nexamples:\n\n  # the base's own\n  - from: a/x\n    to: b1\n    expect: allowed\n  - {}\nrules: {{}}\n",
                example("b2")
            ),
        );
        write(
            dir.path(),
            "easement.yml",
            &format!(
                "extends: base.yml\nexamples:\n- {}\n- {}\n",
                example("c1"),
                example("d1")
            ),
        );
        let policy = Policy::load(&dir.path().join("easement.yml"), None).unwrap();
        let found: Vec<(String, &str, usize)> = policy
            .config
            .examples
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let (input, line) = policy.example_at(i).unwrap();
                (
                    e.to.clone().unwrap(),
                    input.path.file_name().unwrap().to_str().unwrap(),
                    line,
                )
            })
            .collect();
        let at = |to: &str, file, line| (to.to_string(), file, line);
        assert_eq!(
            found,
            [
                // A flow sequence has no item lines: its key's.
                at("d1", "deep.yml", 2),
                at("d2", "deep.yml", 2),
                at("b1", "base.yml", 8),
                at("b2", "base.yml", 11),
                at("c1", "easement.yml", 3),
                // The same example twice is kept twice: each has its own line.
                at("d1", "easement.yml", 4),
            ]
        );
    }

    #[test]
    fn extends_merges_by_name_and_detects_loops() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "base.yml",
            "version: 1
components:
  a: { in: a }
  b: { in: b }
common: [b]
exclude: [gen]
target_only: ['**/*.gen.ts']
tests: auto
rules:
  a: { may_depend_on: [b], because: base }
",
        );
        write(
            dir.path(),
            "easement.yml",
            "extends: base.yml
components:
  c: { in: c }
  b: { in: other/b }
common: [c, b]
exclude: [vendor]
target_only: ['**/*.pb.go']
rules:
  a: { because: own }
",
        );
        let policy = Policy::load(&dir.path().join("easement.yml"), None).unwrap();
        let c = &policy.config;
        assert_eq!(c.components.len(), 4);
        assert_eq!(c.components["b"].patterns.as_vec(), ["other/b"]);
        assert_eq!(c.common, ["b", "c"]);
        assert_eq!(c.exclude, ["gen", "vendor"]);
        assert_eq!(c.target_only, ["**/*.gen.ts", "**/*.pb.go"]);
        // The rule of the extending file replaces the base one as a whole.
        assert_eq!(c.reason("a"), Some("own"));
        assert!(c.rules["a"].may_depend_on.is_empty());
        let kinds: Vec<(InputKind, &str)> = policy
            .inputs
            .iter()
            .map(|i| (i.kind, i.path.file_name().unwrap().to_str().unwrap()))
            .collect();
        assert_eq!(
            kinds,
            [
                (InputKind::Config, "easement.yml"),
                (InputKind::Extended, "base.yml")
            ]
        );
        let from = |key: &str, name: &str| {
            let input = policy.defined_by(key, name).unwrap();
            input
                .path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(from("components", "a"), "base.yml");
        assert_eq!(from("components", "b"), "easement.yml");
        assert_eq!(from("rules", "a"), "easement.yml");
        // `tests: auto` makes its component where it is set.
        assert_eq!(from("components", "tests"), "base.yml");
        // An item of a merged list is where it was first written, though the
        // extending file writes the key last.
        let common = |name: &str, at| policy.provenance(&Site::of_common(name, at));
        assert_eq!(
            common("b", 0),
            Provenance {
                field: "common[0]".to_string(),
                input: Some("base.yml".to_string())
            }
        );
        assert_eq!(common("c", 1).input.as_deref(), Some("easement.yml"));

        write(dir.path(), "loop.yml", "extends: loop.yml\nversion: 1\n");
        let err = Policy::load(&dir.path().join("loop.yml"), None).unwrap_err();
        assert!(err.to_string().contains("extends itself"));
    }

    #[test]
    fn a_policy_at_a_revision_has_the_entries_of_its_extended_files_then() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let shared = tmp.path().join("shared");
        write(
            &shared,
            "outside.yml",
            "components:\n  far: { in: far, owner: outside }\n",
        );
        write(
            &repo,
            "policy/base.yml",
            "version: 1\ncomponents:\n  a: { in: a, owner: old }\n  b: { in: b }\nrules:\n  a: { may_depend_on: [b] }\n",
        );
        write(
            &repo,
            "easement.yml",
            "extends: [policy/base.yml, ../shared/outside.yml]\ncomponents:\n  c: { in: c }\n",
        );
        git(&repo, &["init", "-q"]);
        commit(&repo, "first");
        write(
            &repo,
            "policy/base.yml",
            "version: 1\ncomponents:\n  a: { in: a, owner: new }\n  b: { in: b }\nrules:\n  a: { because: alone }\n",
        );
        commit(&repo, "second");

        let config = repo.join("easement.yml");
        let old = Policy::load(&config, Some("HEAD~1")).unwrap();
        assert_eq!(old.config.components["a"].owner.as_deref(), Some("old"));
        assert_eq!(old.config.rules["a"].may_depend_on.len(), 1);
        let file = |input: &PolicyInput| {
            crate::relpath::slash(input.path.strip_prefix(real(tmp.path())).unwrap())
        };
        let defined = |key: &str, name: &str| file(old.defined_by(key, name).unwrap());
        assert_eq!(defined("components", "a"), "repo/policy/base.yml");
        assert_eq!(defined("rules", "a"), "repo/policy/base.yml");
        assert_eq!(defined("components", "c"), "repo/easement.yml");
        assert_eq!(defined("components", "far"), "shared/outside.yml");
        let past_unknown: Vec<(String, bool)> = old
            .inputs
            .iter()
            .map(|i| (file(i), i.past_unknown))
            .collect();
        assert_eq!(
            past_unknown,
            [
                ("repo/easement.yml".to_string(), false),
                ("repo/policy/base.yml".to_string(), false),
                ("shared/outside.yml".to_string(), true),
            ]
        );

        // Reached through a link, `..` still leaves the real directory.
        #[cfg(unix)]
        {
            std::fs::create_dir(tmp.path().join("elsewhere")).unwrap();
            let link = tmp.path().join("elsewhere/link");
            std::os::unix::fs::symlink(&repo, &link).unwrap();
            let linked = Policy::load(&link.join("easement.yml"), Some("HEAD~1")).unwrap();
            assert_eq!(linked.inputs, old.inputs);
        }

        let now = Policy::load(&config, None).unwrap();
        assert_eq!(now.config.components["a"].owner.as_deref(), Some("new"));
        assert_eq!(now.config.reason("a"), Some("alone"));
        assert!(now.inputs.iter().all(|i| !i.past_unknown));
    }

    #[test]
    fn a_policy_at_a_revision_is_judged_against_the_diagram_then() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        write(
            repo,
            "easement.yml",
            "version: 1\ncomponents:\n  web: { in: web }\n  db: { in: db }\ndiagram: docs/arch.md\n",
        );
        write(
            repo,
            "docs/arch.md",
            "```mermaid\nflowchart LR\n  web --> db\n```\n",
        );
        git(repo, &["init", "-q"]);
        commit(repo, "first");
        write(
            repo,
            "docs/arch.md",
            "```mermaid\nflowchart LR\n  web\n  db\n```\n",
        );
        commit(repo, "drop the arrow");

        let edges = [crate::check::Edge {
            from: "web".to_string(),
            to: "db".to_string(),
            file: "web/a.ts".to_string(),
            line: 1,
        }];
        let config = repo.join("easement.yml");
        let old = Policy::load(&config, Some("HEAD~1")).unwrap();
        assert!(crate::diagram::check(&old, &edges).unwrap().is_empty());
        let now = Policy::load(&config, None).unwrap();
        let violations = crate::diagram::check(&now, &edges).unwrap();
        assert_eq!(violations.len(), 1);
        assert!(
            violations[0]
                .message
                .starts_with("web -> db is not in the diagram")
        );
    }

    #[test]
    fn a_diagram_that_cannot_be_read_fails_only_where_it_is_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "easement.yml",
            "version: 1\ncomponents:\n  a: { in: a }\ndiagram: docs/arch.md\n",
        );
        let policy = Policy::load(&dir.path().join("easement.yml"), None).unwrap();
        let err = format!("{:#}", policy.diagram().unwrap_err());
        assert!(
            err.starts_with("cannot read the diagram docs/arch.md: "),
            "{err}"
        );
        assert_eq!(policy.inputs.len(), 1);
    }
}
