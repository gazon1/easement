# Symbol uses as dependencies on the defining component

Assessed on 2026-11-05 for [issue #103](https://github.com/mokevnin/easement/issues/103), against easement at `a4bd04e`. Sections 1–3 are facts: indexer output observed on fixtures, and tool documentation or source. Section 4 holds the recommendations and is the only place they appear. Anything not observed or read is marked **unverified**.

## The question

Issue #103 asks for an opt-in mode in which a used symbol counts as a dependency on the component that defines it. `gw` writes `chunk::Block`, `chunk` has `pub use blob::Block;`, and the mode judges that as `gw -> blob`. Today [`facts_watching`](../../src/scip.rs#L665) makes a `Dep` only from a module reference. A member reference becomes a `SymbolRef`, and only `forbid_symbol` and `public_symbols` read those. Three things decide whether the mode can be built once for every language:

1. For a name reached through a re-export, what does each indexer emit? Does the reference point at the original, at the re-export site, or at both? Is the re-export line recorded, and with which role?
2. When a member is used on a value of inferred type (`chunk::make().size()`), is it emitted as a reference into the defining component? Can such a reference be told apart from a name the code writes?
3. How do established boundary tools treat re-exports?

## Method

Each language got a fixture of the same shape: `blob` defines `Block` (a field `len`/`Len` and a method `size`), `chunk` re-exports it in every form the language has, and `gw` uses it. Each fixture was indexed with the command easement runs ([`INDEXERS`](../../src/indexer.rs#L174)). The occurrences were dumped with a small decoder built on the `scip` 0.10 crate easement uses. `easement check` and `easement why gw blob`, built from `a4bd04e`, then ran against a config that allows `gw -> chunk -> blob` but not `gw -> blob`. The fixtures and the decoder were throwaway and are not in the repository. The tables below quote the symbols they produced.

| Indexer | Version observed | Pinned in `mise.toml` |
| --- | --- | --- |
| rust-analyzer | 1.97.0 (2d8144b7 2026-07-07) | `rust` 1.97.0 (the issue used 1.99.0) |
| scip-typescript | 0.6.1 | 0.6.1 |
| scip-python | 0.6.8 | 0.6.8 |
| scip-go | 0.2.9 | 0.2.9 |
| scip-ruby | 0.5.1 (Sorbet 0.6) | `scip-ruby-v0.5.1` |
| scip-java | reports `0.0.0-SNAPSHOT` in `tool_info` | 0.13.2 |

PHP and C# were not run. Section 2 states what their code path implies; the claim is **unverified** against an index. Also not tested:

- re-export chains across more than one component (`gw -> chunk -> mid -> blob`);
- Rust `Deref`;
- the indexers' source code. The behaviour described here is what these versions emitted, not what their code guarantees.

Roles are the `symbol_roles` bitset of [`scip.proto`](https://github.com/scip-code/scip/blob/bbec5345bfae602b31286a9ec74e2c2042713f50/scip.proto#L524-L545): 0 is a plain reference, `0x2` is `Import`, `0x8` is `ReadAccess`. Descriptor suffixes follow the [grammar](https://github.com/scip-code/scip/blob/bbec5345bfae602b31286a9ec74e2c2042713f50/scip.proto#L163-L169): `/` namespace, `#` type, `.` term, `().` method. Symbols below are shortened to the package and descriptors.

## 1. Facts: names reached through a re-export

### What the use site references

| Language | Re-export form | Reference at the `gw` use | Re-exporter visible at the use? |
| --- | --- | --- | --- |
| Rust | `pub use blob::Block;` | `cargo chunk crate/` and `cargo blob Block#`, same line | yes: `chunk crate/` on the same line |
| Rust | `pub use blob::Block as Brick;` | `blob Block#` (no `Brick` symbol exists) | yes |
| Rust | `pub use deep::*;` | `chunk crate/` and `deep Star#`, `deep starfn().` | yes |
| Rust | `pub mod api { pub use blob::Block; }` | `chunk crate/`, `chunk api/`, `blob Block#` | yes |
| Rust | `pub use blob as b;` (module) | `chunk crate/`, **`blob crate/`**, `blob Block#` | yes; and `blob crate/` is a module reference |
| Rust | `pub type Alias = blob::Block;` | `chunk Alias#` only (kind `TypeAlias`, no relationship) | the original is hidden |
| TypeScript | `export { Block } from "../blob"` | `src/blob/index.ts/Block#` at the import and at each use | only on the import line (`"../chunk"` → `src/chunk/index.ts/`) |
| TypeScript | `export { Block as Brick, helper }` | `blob …/Block#`, `blob …/helper().` | import line only |
| TypeScript | `export * from "../deep"` | `src/deep/index.ts/Star#`, `…/starfn().` | import line only |
| TypeScript | `import { Block } …; export { Block }` | `blob …/Block#` | import line only |
| TypeScript | `export * as ns from "../blob"` | **`src/blob/index.ts/`** (the module) for `ns`, then `blob …/Block#` | import line only |
| TypeScript | `export type Alias = B2` / `export const C = Block` | `chunk …/Alias#` / `mid …/constAlias.` | the original is hidden |
| Python | `from app.blob import Block` in `chunk/__init__.py`, then `from app.chunk import Block` | `` `app.blob`/Block# `` at the import and at each use | import line only (`` `app.chunk`/__init__: ``) |
| Python | `from app.deep import *` | `` `app.deep`/Star# ``, `` `app.deep`/starfn(). `` | import line only |
| Python | `from app import blob as blobmod`, then `gw` uses `blobmod.Block` | `` `app.blob`/Block# `` only; **no occurrence at all** for `blobmod`, neither in `gw`'s import nor at the use | import line only |
| Python | `Alias = Block` | `` `app.chunk`/Alias. `` (no kind, no relationship) | the original is hidden |
| Go | `type Block = blob.Block` (the only way to re-export) | `` `…/chunk`/ `` and `` `…/chunk`/Block# `` (kind `TypeAlias`, no relationship) | the original is hidden |
| Go | `var Helper = blob.Helper` | `` `…/chunk`/Helper. `` (kind `Variable`) | the original is hidden |
| Ruby | `Block = Blob::Block` inside `module Chunk` | `Chunk#`, `Chunk#Block.`; but `.new` → `Blob#Block#initialize().`, `.build` → ``Blob#`<Class:Block>`#build().`` | the alias is hidden for a bare constant; methods reach the original |
| Java | none in the language | n/a | n/a |

Taken together:

- **Rust, TypeScript and Python point the use straight at the original definition.** The re-exporter has no symbol of its own for the name. Renames and globs behave the same way. The only re-export the use site does not see through is an alias with its own definition: `type`, `export type`, a `const`, or a Python assignment.
- **Go and Ruby can re-export only through an alias that is its own definition**, so the use site names the re-exporter's symbol. Only scip-ruby relates the alias to the original: `Chunk#Block.` carries a `Relationship { symbol: Blob#Block#, is_reference: true }` ([spec](https://github.com/scip-code/scip/blob/bbec5345bfae602b31286a9ec74e2c2042713f50/scip.proto#L465-L515)). For scip-go's `TypeAlias` the relationships are empty, and so are those of rust-analyzer's and scip-typescript's `TypeAlias` and scip-python's `Alias.`. Whether the alias's `signature_documentation` names the target was not checked (**unverified**).
- **Roles never mark an import or a re-export.** No indexer set `Import` (`0x2`) on any occurrence. Rust, TypeScript and the Java references are role 0. Python and Go use `ReadAccess` for every reference. Ruby's roles are mixed: `Chunk#` is 0 at `gw.rb` L4 and `ReadAccess` at L12, and method calls are 0. Whether a name is written or inferred has nothing to do with the role.

### The re-export line in the re-exporter

| Language | Occurrences on the re-export line in `chunk` |
| --- | --- |
| Rust `pub use blob::Block;` | `blob crate/` (0), `blob Block#` (0). Nothing defines `chunk::Block`. |
| Rust `pub use deep::*;` | `deep crate/` (0) only: no per-name occurrence |
| TypeScript `export { Block } from "../blob"` | `blob …/Block#` (0), `src/blob/index.ts/` (0) on the module string |
| TypeScript `export * from "../deep"` | `src/deep/index.ts/` (0) only |
| Python `from app.blob import Block` | `` `app.blob`/__init__: `` (ReadAccess), `` `app.blob`/Block# `` (ReadAccess) |
| Python `from app.deep import *` | `` `app.deep`/__init__: `` only |
| Go `type Block = blob.Block` | Definition `` `…/chunk`/Block# `` with an enclosing range, `` `…/blob`/ `` and `` `…/blob`/Block# `` (ReadAccess) |
| Ruby `Block = Blob::Block` | Definition `Chunk#Block.`, `Blob#` (0), `Blob#Block#` (ReadAccess) |

In every language the re-export line is recorded. It carries a reference to the original symbol, or for a glob to the original module, plus a module reference to the defining module. Nothing in its roles sets it apart from any other use of the name inside `chunk`. In the fixture, `chunk`'s `make()` references `blob Block#` the same way.

### What easement does today (observed)

| Fixture | `easement check` | Line that fails |
| --- | --- | --- |
| Rust | **fails** only for `pub use blob as b;`, at `gw/src/lib.rs:14` (`chunk::b::Block`), because `blob crate/` is a module reference. Without it the fixture passes, as in #103. | 14 |
| TypeScript | **fails** only for `export * as ns`, at the `import { ns } from "../chunk"` line (`src/blob/index.ts/` is a module reference). Moved into its own file, `index.ts` with every named and glob re-export **passes**. | `src/gw/ns.ts:1` |
| Python | passes | none |
| Go | passes | none |
| Ruby | **fails** at `gw.rb:4` (`Chunk::Block.new`): `Blob#Block#initialize().` becomes its class `Blob#Block#` and so a `Dep` | 4 |
| Java | **fails** at `Gw.java:9` (`b.size()` on `var b = Chunk.make()`): `fx/blob/Block#size().` becomes its class | 9 |

The two failures in Ruby and Java come from [`scip.rs` L790](../../src/scip.rs#L790): "For PHP, Java, C# and Ruby a member (a method, a constructor) is a use of its class". [`class_symbol`](../../src/scip.rs#L348) cuts the member off, the class is a `Referent::Module`, and the reference becomes a `Dep` on the class's file. By the same code path PHP and C# behave this way too (**unverified** by an index). A whole-module re-export therefore already counts in Rust and TypeScript, and so does any member use in the four class-based languages. A name re-export in Rust, TypeScript or Python does not count, and neither does a Go or Ruby alias.

## 2. Facts: members on values of inferred type

| Language | `b := chunk.make(); b.size(); b.len` and `chunk.make().size()` | Inherited/promoted member | Signal on the occurrence |
| --- | --- | --- | --- |
| Rust | `blob impl#[Block]size().` (kind `Method`), `blob Block#len.` (kind `Field`) | not tested (`Deref` would be the case) | none |
| TypeScript | `src/blob/index.ts/Block#size().`, `…/Block#len.` | `class Own extends B2`, then `o.size()` → `blob …/Block#size().`; `Own#` has `Relationship { Block#, is_implementation }` | none |
| Python | `` `app.blob`/Block#size(). ``, `` `app.blob`/Block#len. `` | not tested | none; no `SymbolInformation.kind` |
| Go | `` `…/blob`/Block#Size(). ``, `` `…/blob`/Block#Len. `` | struct embedding `blob.Block`, then `e.Size()` / `e.Len` → `` `…/blob`/Block#Size(). `` / `Block#Len.`; a composite literal `chunk.Block{Len: 2}` → `blob Block#Len.` though the written type is `chunk/Block#` | none |
| Ruby | emitted **only when Sorbet knows the type**: with `sig { returns(Blob::Block) }` on `make`, `b.size` → `Blob#Block#size().`; untyped, nothing | n/a | none; no kind |
| Java | `fx/blob/Block#size().`, `fx/blob/Block#len.` | `Sub extends fx.blob.Block`, then `s.size()` → `fx/blob/Block#size().` | none |

Observed signals:

- **Roles:** the same as for a written name (table above). There is no distinction.
- **`syntax_kind`:** none of the six indexers set it on any occurrence in these fixtures. It is optional and meant for highlighting ([spec L738-L739](https://github.com/scip-code/scip/blob/bbec5345bfae602b31286a9ec74e2c2042713f50/scip.proto#L738-L739)).
- **Enclosing range:** no reference into another file carried one in any of the six indexes. rust-analyzer gives a same-file reference the span of the definition it points at (`blob/src/lib.rs` L2 `Block#`: `[0, 0, 35]`), not the span of an enclosing node at the use. scip-typescript, scip-python, scip-go, scip-ruby and scip-java set it only on definitions.
- **`SymbolInformation.kind`** (`Method`, `Field`, `Property`, `TypeAlias`): present in rust-analyzer, scip-typescript and scip-go. Absent in scip-python and scip-ruby. It describes the symbol, not the occurrence.
- **Descriptor shape** is the one signal that holds in every observed index. A name the code writes through a module path is *top-level*: namespaces, then one type, term or method (`blob Block#`, `blob helper().`, `` `app.blob`/Block# ``). A member is a term or method whose preceding descriptor is a type (`Block#len.`, `impl#[Block]size().`, `Blob#Block#size().`). Ruby needs its own reading, which easement already applies, because its modules are types too (`Blob#Block#` is a constant path).

What a "top-level names only" filter would lose:

- a Go composite literal's fields (`chunk.Block{Len: 2}`), the one place a Go alias leaks `blob`;
- Ruby `.new` and class methods on an aliased constant;
- an associated function written with its type (`chunk::Block::new()`). This one stays covered, because rust-analyzer also emits `blob Block#` on that line.

What a "members too" mode would add: inferred, inherited and promoted uses. That is the noise #103 wants to avoid, and it is also exactly what Java, Ruby, PHP and C# count today.

## 3. Facts: how other boundary tools treat re-exports

| Tool | Unit judged | Re-exports | Transitive or defining-site option |
| --- | --- | --- | --- |
| dependency-cruiser | module (file) by resolved import path | `export { thing } from "./things"` is a dependency of the barrel, type `export` ([rules reference, dependency types](https://github.com/sverweij/dependency-cruiser/blob/main/doc/rules-reference.md#dependencytypes-and-dependencytypesnot)). Neither the rules nor the options reference mentions following a name to its defining module; the only "re-export" in the options reference is `detectImportsInAmbientModules`. | `reachable` "(either directly or via other modules)" ([reachable](https://github.com/sverweij/dependency-cruiser/blob/main/doc/rules-reference.md#reachable---detecting-dead-wood-and-transient-dependencies)) |
| eslint-plugin-boundaries | file by import source | `boundaries/dependency-nodes: ['export']` "Analyze `export ... from` statements, including `export type`" ([settings](https://www.jsboundaries.dev/docs/setup/settings/)), so the barrel's re-export is the barrel's dependency | none found |
| Nx `@nx/enforce-module-boundaries` | project by import path | not mentioned in the [rule options](https://nx.dev/docs/kb/enforce-module-boundaries) (`depConstraints`, `banTransitiveDependencies`, `checkNestedExternalImports`, …) | `banTransitiveDependencies` concerns npm packages missing from `package.json`, not re-exports |
| import-linter / grimp (Python) | module: "a directed graph of imports between modules" ([grimp usage](https://github.com/seddonym/grimp/blob/main/docs/usage.rst)); whether `from pkg import X` records `pkg` or the module that defines `X` is **unverified** | not mentioned | `forbidden` and `layers` check indirect chains by default; `allow_indirect_imports: True` turns it off ([forbidden](https://github.com/seddonym/import-linter/blob/main/docs/contract_types/forbidden.md)). Closed layers stop "reaching through" ([grimp, closed layers](https://github.com/seddonym/grimp/blob/main/docs/usage.rst#closed-layers)). |
| ArchUnit (Java) | class, from bytecode: no re-exports exist | n/a | A `Dependency` includes "a class calls a method/constructor of another class" and "accesses a field of another class", built from the access's target owner ([`Dependency.java` L42-L89](https://github.com/TNG/ArchUnit/blob/main/archunit/src/main/java/com/tngtech/archunit/core/domain/Dependency.java#L42-L89)). So member uses count, like easement's class-based languages. `getTransitiveDependenciesFromSelf` exists. Whether the owner of an inherited call is the subclass (javac's static receiver type) or the declaring class is **unverified**. |
| go-arch-lint | package by import path | not mentioned | `deepScan` ("advanced AST code analyse", default `true` since v3, [syntax](https://github.com/fe3dback/go-arch-lint/blob/master/docs/syntax/README.md)) judges the package of an implementation injected into an interface parameter ([searcher.go](https://github.com/fe3dback/go-arch-lint/blob/master/internal/services/checker/deepscan/searcher.go#L63-L76)). It judges a definition site, but for DI, not for aliases. |
| depguard | import path prefix | not mentioned: "checks package imports are in a list of acceptable packages" ([README](https://github.com/OpenPeeDeeP/depguard/blob/v2/README.md)) | none |
| cargo-modules | item graph with `use` edges (`--no-uses` filters them) ([README](https://github.com/regexident/cargo-modules)) | whether a `pub use` edge resolves to the original item is **unverified** | n/a; it is a visualizer, not a gate |
| Sonargraph | **not reviewed** | | |

None of the reviewed tools has an option that judges a re-exported name by its defining module. JS and TS tools treat the barrel's `export … from` as the barrel's own dependency, which is easement's current reading. Python's import-linter catches the case only as an indirect chain (`gw -> chunk -> blob`). That is easement's `must_not_reach`, and it has the same false positive #103 reports: a `gw` that uses only `chunk::Own` still has the chain. The only tools that judge where a used symbol lives work on JVM bytecode (ArchUnit), where re-exports do not exist and member access is the dependency.

## 4. Implications for #103 (recommendations)

Everything below is a recommendation drawn from the facts above, not a fact.

**Which languages can support the mode.**

- **Rust, TypeScript, Python: yes, directly.** The use site already references the original definition, and its home file is in `defined`. The mode only has to let a top-level member reference with an internal home make a dependency. The aliases each language allows (`type`, `export type`, `Alias = Block`) stay hidden. That is defensible as "the re-exporter made a new name", but it should be documented as a limit.
- **Go and Ruby: only by following aliases.** The use site names the re-exporter's own symbol. Ruby states the target in a relationship (`is_reference`), so it can be followed from the index alone. Go states nothing: following `type Block = blob.Block` would mean reading the references inside the alias's definition range, which is a heuristic. Two options: ship Go as an evidence gap of this rule, or agree on the alias-following mechanism for every language in the issue first, as AGENTS.md requires for a mechanism that does not fit.
- **Java, PHP, C#:** Java has no re-exports. These languages already count member uses as class dependencies, so their result for the case in #103 does not change. The mode would be a no-op there, unless the rule's definition changes what they count today.
- Kotlin and Swift (tree-sitter) have no symbol uses. The mode would be an evidence gap there, as `forbid_symbol` already is.

**Excluding inferred-type member uses.** No occurrence-level signal exists: not roles, not `syntax_kind`, not an enclosing range. The descriptor shape (top-level versus a member under a type) is the only signal common to all observed indexers, and it is reliable as a fact about the symbol. Excluding members is therefore feasible: count only top-level names. It costs Go's literal fields and Ruby's `.new` on an alias. Counting members makes the mode equal to the class-based behaviour and pulls in inherited and promoted uses, which were confirmed in TypeScript, Go and Java. One cross-language choice has to be made. Today the languages already disagree: members count in Java, PHP, C# and Ruby, and do not in Rust, TypeScript, Python and Go. A "rule defined once for every language" should state which reading it takes, and the class-based languages' current behaviour should be recorded in the issue as an existing inconsistency, whatever #103 decides.

**The re-export line as a witness hop for `--diff`.** The witness is available but has to be searched for. Diff mode keeps a violation when any `evidence` line changed ([`check.rs` L154-L156](../../src/check.rs#L154-L156), [`diff.rs` L132](../../src/diff.rs#L132)), so the violation should carry two lines: `gw`'s use line and the re-export line. No role marks the re-export line, and the first reference to the symbol need not be it: in the fixture, `chunk`'s `make()` references `blob Block#` exactly as the `pub use` line does. Put `make()` above a newly added `pub use` and a single "first line" witness would miss the change. `evidence` is a list, and `diff.keeps` matches any entry, so carry every line of the re-exporting component's files that references the symbol, plus `gw`'s use line. For a glob, carry the lines that reference the original module. For Go or Ruby, carry the alias's definition line. This over-reports rather than misses: a `--diff` that changes an unrelated use of `Block` inside `chunk` also reports the `gw` violation. That fits the repository's "when in doubt, miss the cache" stance better than a dropped violation does. The re-exporting component is the one whose module the use line or the import line references:

- Rust puts `chunk crate/` on the same line as the use;
- TypeScript and Python put it only on the import line.

The existing `chunk -> blob` `Dep` cannot serve as the witness either: it is deduplicated to the first line per file and target, which need not be the re-export line.

**Smaller points for the issue.**

- The mode should judge internal targets only (`Target::Internal`). External and standard-library symbols then need no exemption list. `common` components still need the same exemption `may_depend_on` gives them.
- The message #103 proposes, "uses blob::Block, re-exported by chunk", is supported by the data in Rust, TypeScript and Python through the import line, and in Go and Ruby through the alias.
- The precedents (whole-module re-exports already counting in Rust and TypeScript, member uses counting in the class-based languages) suggest the asymmetry #103 describes is narrower than "dependencies are imports". That is an argument for discussing in the issue whether the mode is opt-in or the default.
