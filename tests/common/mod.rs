//! What the integration tests share: scratch copies of the fixture projects.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

pub fn copy_dir(from: &Path, to: &Path) {
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

/// Writes a stub package for each import specifier into `node_modules`, so the
/// TypeScript indexer sees the module as installed: `lodash/fp` becomes
/// `node_modules/lodash/package.json` and `node_modules/lodash/fp.d.ts`.
pub fn stub_packages(root: &Path, specifiers: &[&str]) {
    for specifier in specifiers {
        let mut parts = specifier.split('/');
        let first = parts.next().unwrap();
        let (name, rest): (String, Vec<&str>) = if first.starts_with('@') {
            (
                format!("{first}/{}", parts.next().unwrap()),
                parts.collect(),
            )
        } else {
            (first.to_string(), parts.collect())
        };
        let dir = root.join("node_modules").join(&name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("package.json"),
            format!(r#"{{"name":"{name}","version":"1.0.0","types":"index.d.ts"}}"#),
        )
        .unwrap();
        let stub = "declare const stub: any;\nexport default stub;\n";
        fs::write(dir.join("index.d.ts"), stub).unwrap();
        if !rest.is_empty() {
            let file = dir.join(format!("{}.d.ts", rest.join("/")));
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, stub).unwrap();
        }
    }
}

/// A scratch copy of a fixture project, so tests can write baselines and edit code.
/// A `modules.txt` in the fixture lists third-party imports to stub as installed.
pub fn fixture(name: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    copy_dir(&src, tmp.path());
    let root = tmp.path().to_path_buf();
    if let Ok(list) = fs::read_to_string(root.join("modules.txt")) {
        stub_packages(&root, &list.lines().collect::<Vec<_>>());
        fs::remove_file(root.join("modules.txt")).unwrap();
    }
    (tmp, root)
}
