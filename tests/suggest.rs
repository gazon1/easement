//! `suggest --patch` through the library: the patch it writes applies.

use std::fs;

use easement::commands;
use easement::{Cache, CacheUse, Environment, Indexing, Jobs};

/// The patch names the config by its path in the project, however the config
/// was given (the binary passes `--config` as written, here it is absolute),
/// so `git apply` in the root of the project applies it.
#[test]
fn the_patch_of_suggest_applies_whatever_path_names_the_config() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    for (path, text) in [
        (
            "easement.yml",
            "version: 1\ncomponents:\n  app: { in: src/app }\n  old: { in: src/old }\nrules:\n  app:\n    may_depend_on: [old]\n",
        ),
        ("tsconfig.json", "{\"include\":[\"**/*\"]}\n"),
        ("src/app/a.ts", "export const a = 1;\n"),
        ("src/old/o.ts", "export const o = 1;\n"),
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let config = tmp.path().join("easement.yml");
    let mut cache = Cache::new(uncached());
    let diff = commands::suggest_patch(&config, &mut cache).unwrap().diff;
    assert!(
        diff.contains("--- a/easement.yml\n+++ b/easement.yml\n"),
        "{diff}"
    );
    fs::write(root.join("fix.patch"), &diff).unwrap();
    let applied = std::process::Command::new("git")
        .args(["apply", "fix.patch"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(applied.success());
    let fixed = fs::read_to_string(root.join("easement.yml")).unwrap();
    assert!(!fixed.contains("old]"), "{fixed}");
}

/// Indexing with no cache, by indexers that run in the environment of the tests.
fn uncached() -> Indexing {
    Indexing {
        cache: CacheUse::Off,
        cache_dir: None,
        environment: Environment::of_process(),
        jobs: Jobs::default(),
    }
}
