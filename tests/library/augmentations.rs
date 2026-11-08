//! An augmentation of an external name (#134): `app` augments the package
//! `router` (`declare module "router" { … }`), and `platform` may depend on nothing.
//! The index points a name of `router` both at the package and at the augmentation;
//! the name the code writes is the package's, so writing it is no dependency on `app`.
//! A name that only the augmentation defines is `app`'s, as any name defined there is.

use std::path::PathBuf;

use tempfile::TempDir;

use crate::common::project;
use crate::run::{check, found, json};

/// A project whose `app` augments the installed package `router`, which defines
/// `Register` and `Link`; the augmentation adds `extra` and imports `server`.
fn augmented(files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let mut all = vec![
        (
            "easement.yml",
            "version: 1\ncomponents:\n  platform: { in: src/platform, optional: true }\n  app: { in: src/app }\n  server: { in: src/server }\n  ui: { in: src/ui, optional: true }\nrules:\n  platform: { may_depend_on: [] }\n  app: { may_depend_on: [platform, server] }\n  ui: { may_depend_on: [app], must_not_reach: [server] }\n",
        ),
        (
            "package.json",
            "{\"name\": \"app\", \"dependencies\": {\"router\": \"1\"}}\n",
        ),
        (
            "node_modules/router/package.json",
            "{\"name\": \"router\", \"version\": \"1.0.0\", \"types\": \"index.d.ts\"}\n",
        ),
        (
            "node_modules/router/index.d.ts",
            "export interface Register {}\nexport declare function Link(): void;\n",
        ),
        ("src/server/db.ts", "export const db = 1;\n"),
        (
            "src/app/router.ts",
            "import { db } from \"../server/db\";\n\ndeclare module \"router\" {\n  interface Register {\n    router: typeof db;\n  }\n  export function extra(): void;\n}\n\nexport {};\n",
        ),
    ];
    all.extend_from_slice(files);
    project(&all)
}

#[test]
fn a_name_of_an_augmented_package_is_no_dependency_on_the_augmentation() {
    let (_tmp, root) = augmented(&[
        ("src/platform/link.ts", "export { Link } from \"router\";\n"),
        (
            "src/platform/register.ts",
            "import type { Register } from \"router\";\nexport type R = Register;\n",
        ),
    ]);
    let report = json(&check(&root));
    assert_eq!(found(&report), [], "{report}");
}

#[test]
fn a_component_reaches_nothing_through_an_augmentation_of_a_package_it_imports() {
    // The augmentation imports `server`; `page.ts` imports only the package, while
    // `direct.ts` imports the augmenting file itself and so reaches `server`.
    let (_tmp, root) = augmented(&[
        (
            "src/ui/page.ts",
            "import { Link } from \"router\";\nexport const page = Link;\n",
        ),
        (
            "src/ui/direct.ts",
            "import \"../app/router\";\nexport const d = 1;\n",
        ),
    ]);
    let report = json(&check(&root));
    assert_eq!(
        found(&report),
        [(
            "src/ui/direct.ts".to_string(),
            1,
            "ui -> server".to_string()
        )],
        "{report}"
    );
}

#[test]
fn a_name_that_only_a_project_file_defines_stays_a_dependency_on_it() {
    let (_tmp, root) = augmented(&[
        // Only the augmentation defines `extra`.
        (
            "src/platform/extra.ts",
            "import { extra } from \"router\";\nexport const e = extra;\n",
        ),
        // Only `shim.d.ts` declares `virtual:thing`.
        (
            "src/app/shim.d.ts",
            "declare module \"virtual:thing\" {\n  export const x: number;\n}\n",
        ),
        (
            "src/platform/virtual.ts",
            "import { x } from \"virtual:thing\";\nexport const v = x;\n",
        ),
        (
            "src/platform/direct.ts",
            "import \"../app/router\";\nexport const d = 1;\n",
        ),
    ]);
    let report = json(&check(&root));
    let mut violations = found(&report);
    violations.sort();
    let expected = |file: &str| (file.to_string(), 1, "platform -> app".to_string());
    assert_eq!(
        violations,
        [
            expected("src/platform/direct.ts"),
            expected("src/platform/extra.ts"),
            expected("src/platform/virtual.ts"),
        ],
        "{report}"
    );
}
