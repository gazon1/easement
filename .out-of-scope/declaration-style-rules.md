# Rules on the shape of a declaration or a file

easement does not check how a declaration is written or how declarations are arranged inside a file: modifiers (`abstract`, `interface`, `final`, `sealed`), one class or widget per file, or helper methods that return a widget. These are a linter's job.

## Why this is out of scope

easement judges where a written name points: which component's file a dependency or a symbol use lands in (ADR 0003). A rule belongs here when it names components and judges where a reference lands. The rules above do neither: they judge the arrangement of code inside one file, or a declaration's own keywords.

The facts are not in the index either. SCIP gives no modifiers: `abstract` or `sealed` shows up only as display text in some indexers (scip-java, scip-php, scip_dart), and is missing from others. `kind` tells an interface from a class in only some languages, and every Dart class is `Class`. A portable rule would have to parse each language's display text, which is what a compiler-backed linter already does properly.

Linters already ship these rules:

- one widget per file: DCM `prefer-single-widget-per-file`, and MIT versions in dart_code_linter, many_lints and saropa_lints;
- no widget-returning helpers: DCM `avoid-returning-widgets`, solid_lints `avoid_returning_widgets`, and the same MIT packages;
- a contract declared as an interface: architecture-test packages such as heimdall_test and arch_dart (`beInterfaces` / `shouldBeInterface`), and ArchUnit, ArchUnitNET, PHPat and Konsist in other languages.

No policy-file architecture tool (Deptrac, Sonargraph, import-linter, dependency-cruiser, go-arch-lint) asserts a one-declaration-per-file rule.

Evidence and sources: `docs/research/gate-or-linter.md`.

## What is still open

These remain open, in #85:

- whether a type that implements a contract must be defined in a given component (placement by relationship);
- whether a type claimed as a contract must be an abstraction rather than an implementation.

Both judge which component something belongs to, so they are not covered by this decision.

## Prior requests

- #85: experimental Dart rules `dart-one-widget-per-file`, `dart-no-widget-helpers` and `dart-repository-declaration`, split out of PR #71
