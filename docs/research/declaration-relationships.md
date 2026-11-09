# Declaration relationships: can the index tie a written supertype to "T implements X"?

Assessed on 2026-11-08 for [issue #85](https://github.com/mokevnin/easement/issues/85), against easement at `f5daf79`. Sections 1–8 hold facts: indexer output observed on fixtures, indexer source at a named tag or commit, the SCIP specification, and upstream issue trackers. Section 9 holds the only conclusions. Anything not observed or read is marked **unverified**. This follows [gate-or-linter.md](gate-or-linter.md) §3, which it re-observes and in places corrects (section 2).

## The question

easement wants to judge who may *implement or extend* a contract of component X, apart from who may *use* it. Two framings are on the table:

- **(A) A qualifier on an existing dependency.** `implements X`, `extends X`, `impl X for T` and `: X` already write X's name, so under [ADR 0003](../adr/0003-a-dependency-is-a-written-name.md) each is already a dependency. The new fact would mark that written occurrence as *providing* X rather than *consuming* it. There are no new edges. Structural satisfaction does not count, since no name is written. In Rust the impl site is judged.
- **(B) A separate graph of declaration relationships** (type T implements or extends type X), built from SCIP `Relationship.is_implementation`, with provenance (declared or structural, direct or transitive), kept apart from dependencies.

(A) needs the index to answer one question **without reading source text**: given T with a relationship to X, which written occurrence of X declares the supertype? In `class Foo implements Bar { Bar copy() { … } }` both `Bar`s lie inside `Foo`'s definition, and only the first is in the supertype clause.

## Method

One fixture per indexer, of one shape where the language allows:

- a contract `Bar` with a method returning `Bar`, `SubBar extends Bar`, a generic `Repository<T>`, `Order`, and an abstract `Base`;
- `Foo extends Base implements Bar, Repository<Order>`, with `Bar` also written as a field type, a return type, a parameter, in a body, and in a nested type;
- `GenFoo implements Repository<Bar>`, where `Bar` is only a type argument;
- `Both implements Bar, Repository<Bar>`;
- `Bnd<T extends Bar> implements Bar`;
- the chain `A extends B`, `B implements C`;
- an external supertype from a dependency or the standard library.

The languages that have them got more: Java records, enums, initializer blocks and annotations; C# primary constructors, structs and records; TypeScript `implements` of a type alias and a structurally matching class; Python `Protocol`; Go accidental satisfaction, struct and interface embedding, generic interfaces and `var _ I = (*T)(nil)`; Rust impls in each module, marker and default-only impls, a supertrait, same-named traits, an associated-type-only impl and a blanket impl; Ruby `include`, `extend`, `prepend` and qualified ancestors; Dart mixins.

Each fixture was indexed with the command easement runs ([`INDEXERS`](../../src/indexer.rs#L181)), run directly in the fixture's root rather than through easement. The indexes were read with `scip snapshot --strict=false` and with a small decoder over `scip print --json`. The **header test** used below is the decoder's, not an indexer's. An occurrence of X passes it for T when all of these hold:

- it lies inside the enclosing range of T's definition;
- it starts after T's name;
- it lies inside no enclosing range of another definition within T. A definition whose range equals T's is ignored, as scip-java gives one to a type parameter.

It is the most a position-only test can do with the ranges each index carries.

The snapshots and decoded excerpts are kept in [`declaration-relationships/`](declaration-relationships/README.md), one directory per indexer, with the manifests and the commands that rebuild each index. The fixture projects themselves are not kept in the repository, because their manifests break easement's own check (that README says why). The README shows how to recover them from the snapshots.

| Indexer | Run | `tool_info` in the index |
| --- | --- | --- |
| scip-java | 0.13.2 (`mise.toml`), JDK temurin-21, Maven 3.10.0 | `0.0.0-SNAPSHOT` |
| scip-typescript | 0.6.1 | `0.6.1` |
| scip-dotnet | 0.2.16 (CI workflow), .NET SDK 10.0.403 | `0.1.0-SNAPSHOT` |
| scip-python | 0.6.8 | `0.6.8` |
| scip-go | 0.2.9, Go 1.27.3 | `0.2.9` |
| rust-analyzer | 1.99.1 (58cf8f9c 2026-10-13) | same |
| scip-php | git `71a5b117` (CI workflow), local PHP 8.5.12 (CI uses 8.4) | `0.0.1` |
| scip-ruby | `scip-ruby-v0.5.1` | `0.5.1` |
| scip_dart | 1.8.1 with analyzer 14.4.0 (PR #71's lockfile), Dart SDK 3.13.7 installed ad hoc | `1.7.0`: the constant in [`lib/src/version.dart` L5](https://github.com/Workiva/scip-dart/blob/fef29da4c46efa3672056ef267ec32a7ea34f41a/lib/src/version.dart#L5) at the 1.8.1 tag was not bumped |

## 1. Facts: what the specification can carry

From [`scip.proto` at `bbec534`](https://github.com/scip-code/scip/blob/bbec5345bfae602b31286a9ec74e2c2042713f50/scip.proto):

- A `Relationship` ([L465-L517](https://github.com/scip-code/scip/blob/bbec5345bfae602b31286a9ec74e2c2042713f50/scip.proto#L465-L517)) belongs to a `SymbolInformation` and holds a target symbol and four booleans: `is_reference`, `is_implementation`, `is_type_definition` and `is_definition`. It has no range and no occurrence. It has no flag for extends versus implements, none for direct versus inherited, and none for declared versus structural. The spec's example (`Dog#` → `Animal#`) uses `is_implementation` for a class implementing an interface.
- An `Occurrence` ([L725-L805](https://github.com/scip-code/scip/blob/bbec5345bfae602b31286a9ec74e2c2042713f50/scip.proto#L725-L805)) has a symbol, `symbol_roles`, `override_documentation`, `syntax_kind` (a highlighting class) and an enclosing range. For a reference, the enclosing range "should indicate the start/end bounds of the parent expression" and "must enclose the occurrence range". That is the one field that could in principle carry a heritage clause. No indexer observed here uses it that way (section 3).

So (A) has no field made for it. Any tie between an occurrence and a relationship has to come from position, or from a role or kind that an indexer happens to set differently.

## 2. Facts: corrections to gate-or-linter.md

- **L106** says easement reads no `SymbolInformation`. It does: [`aliases_of`](../../src/scip/mod.rs#L172-L190) reads `kind == TypeAlias` and relationships with `is_reference` alone, to follow aliases ([ADR 0003](../adr/0003-a-dependency-is-a-written-name.md)).
- **L247** calls placement by relationship "the Deptrac/Sonargraph pattern". Its own §2 table (L76, L82) says those tools only use implements/extends to *select* layer membership.
- **L250** says "scip_dart is transitive, the others direct". Observed here, scip-java, scip-typescript (for classes) and scip-dotnet are transitive too: `A` gets relationships to `B` and to `C`. Only scip-python is direct-only. scip-go's relationships are structural, so the distinction does not apply (section 4).
- **L142** says rust-analyzer "set them [enclosing ranges] on references". The ranges observed are the *referenced* symbol's definition range, and only when that symbol is defined in the same file. `impl Marker for crate::types::Order {}` on line 27 of `contract.rs` gets the range 6:0–7:19 of `trait Marker`, which does not enclose line 27. That breaks the spec's "must enclose" rule. References to symbols in other files get no range ([`rust/decoded.txt`](declaration-relationships/rust/decoded.txt)).
- **L240** left two cases **unverified**: whether an `impl Trait for T {}` with no method leaves a trace, and whether a default-only impl does. Both are now observed: neither leaves one (section 5).
- **L241** left accidental Go satisfaction **unverified**. It is now observed (section 6).
- **L120** left the analyzer version behind scip_dart's signature text **unverified**. PR #71's lockfile resolves analyzer 14.4.0, and the signatures read as that note predicted (section 7).

## 3. Facts: question 1, what marks the supertype occurrence

Two observations hold for every indexer, before the per-indexer table:

- **Roles carry nothing**, except in scip-ruby. Every reference in the scip-java, scip-typescript, scip-dotnet, rust-analyzer and scip_dart indexes has `symbol_roles` 0. Every reference in scip-go and scip-python has `ReadAccess`. The supertype clause is no exception.
- **No reference has a parent-expression enclosing range**, so position is the only remaining signal. The only references with any range are rust-analyzer's, which point at the target's definition (section 2).

`syntax_kind` was set only by scip-php, and only as a highlighting class. `override_documentation` appeared only on scip-ruby's occurrence of an instance-variable assignment.

The supertypes do appear as **display text**, never with a position:

- scip-java's and scip_dart's `signature_documentation`: `public class Foo extends Base implements Bar, Repository<Order>`;
- the hover markdown of scip-python (`class Foo(Base, Bar, Repository[Order]):`) and scip-php;
- scip-ruby's hover, which shows the superclass but not `include`: `class Fx::Foo < Fx::Base`. gate-or-linter.md L119 saw no superclass there; its fixture was different, and the difference is recorded, not resolved.

scip-typescript's and scip-dotnet's class hover is `class Foo` alone, and scip-go's signature is the struct's body. No signature in any index has `Signature.occurrences`, so the text names the supertypes as written but points at no occurrence.

| Indexer | Signal that can single out the supertype occurrence | Verdict | Failing cases observed |
| --- | --- | --- | --- |
| scip-java | Position only, with the header test. Definitions carry enclosing ranges, and those of fields (`4:4-22` over `private Bar field;`), record components (`3:11-20` over `Bar inner`), methods and locals include their types. Header ∩ relationship targets picked `implements Bar` in `Foo`, `Foo.Inner`, `SubBar`, `Rec`, `Kind` and `Tagged`, where the annotation `@Tag(Bar.class)` stands before the name. It rejected `Bar` in `Repository<Bar>`, since `Bar` is not a target of `GenFoo`. | heuristic | `Both implements Bar, Repository<Bar>`: both `Bar`s pass. `Bnd<T extends Bar> implements Bar`: the type parameter's range is the whole class (`7:0-9:1`), so the bound passes too. `static { …Bar.class… }` passes, since an initializer is no definition. An anonymous `new Bar() {…}` has no type symbol or relationship at all: only its method relates to `Bar#copy()`. |
| scip-typescript | Position only. Ranges exist on classes and methods but not on properties, fields, parameters, type parameters or interface members. *Type* relationships exist **only for class declarations**: the code tests `ts.isClassDeclaration`, so interfaces and class expressions get none ([`FileIndexer.ts` L495-L499 at v0.6.1](https://github.com/sourcegraph/scip-typescript/blob/1e6fb387107f065e0e2b22817cbbc86d65289ba2/src/FileIndexer.ts#L495-L499)). Methods, method signatures and properties do get relationships to same-named members of ancestors (L499-L513). | heuristic, classes only | `private field: Bar` and `copy = (): Bar => this` pass. `Both` and the bound in `Bnd` pass. `interface SubBar extends Bar` has **no relationship** to match. Neither has `class ViaAlias implements BarAlias`, because the heritage walk follows only class and interface declarations ([L938-L980](https://github.com/sourcegraph/scip-typescript/blob/1e6fb387107f065e0e2b22817cbbc86d65289ba2/src/FileIndexer.ts#L938-L980)). |
| scip-dotnet | None. No occurrence in the index has an enclosing range, not even a type definition (gate-or-linter.md saw the same), so only the order of occurrences is left. | none | "Between T's name and the next definition" fails both ways. In `private IBar? field` the type precedes the field's definition. In `class Prim(IBar inner) : IBar` the parameter's type precedes the parameter's definition (39:18 against 39:23), and the real supertype (39:32) follows it. |
| scip-python | Position only. Ranges exist on classes and functions, decorators included, but not on annotated class attributes or parameters. | heuristic | `field: Bar` passes, and so does `Both`. For an external base, the written occurrence and the relationship name **different symbols**: `class MySized(collections.abc.Sized)` writes `python-stdlib builtins/Sized#`, while its relationship targets `python-stdlib typing/Sized#`. Matching by symbol fails. |
| scip-go | There is no supertype clause. Satisfaction is structural, so for a struct there is usually no written occurrence to find (section 6). Struct embedding is the exception: `type Embedded struct{ bar.Bar }` puts a reference to `Bar#` on the same range (35:5–8) as the definition of the field `Embedded#Bar.`, and `Embedded` relates to `Bar#`. | none (embedding: one observation) | `var _ bar.Bar = (*Foo)(nil)` is two package-level `ReadAccess` references that nothing ties to `Foo`. An interface embedding `Bar` in `SubBar` writes `Bar` and gets no relationship. |
| rust-analyzer | None. There are no relationships at all (0 in the index). The trait occurrence on `impl Bar for Foo` is a plain reference, and no occurrence or range belongs to the impl block. An impl shows only in the descriptors of the items it defines, as written text (section 5). | none | `impl Marker for Foo {}`, `impl Defaulted for Foo {}`, `impl<T: Bar> Named for T {}` and `#[derive(Clone, Debug)]` leave no impl symbol anywhere. |
| scip-php | None. There are no relationships (0). Every reference has `syntax_kind` `Identifier` (6): the supertype clause, a return type and `instanceof` alike. Type definitions have `IdentifierType` (19). No occurrence has a range. | none | — |
| scip-ruby | **Roles do differ.** The ancestors of a class (`< Base` and `include Bar`) get a reference with `symbol_roles` 0, while every other constant reference is `ReadAccess`. That covers `sig { params(other: Bar) }`, `is_a?(Bar)`, `Bar.instance_method`, the right side of `Alias = Bar`, `extend Bar` and `prepend Bar`. `include` gets both: a role-0 reference and a `ReadAccess` one on the same range. The source agrees: [`typecheckClass`](https://github.com/sourcegraph/scip-ruby/blob/fe92d977da9cec04c964de881b07001f96162f0c/scip_indexer/SCIPIndexer.cc#L2119-L2122) passes each of `klass.ancestors` to [`saveQualifierReferences`](https://github.com/sourcegraph/scip-ruby/blob/fe92d977da9cec04c964de881b07001f96162f0c/scip_indexer/SCIPIndexer.cc#L508-L535) with roles 0, while [constant literals](https://github.com/sourcegraph/scip-ruby/blob/fe92d977da9cec04c964de881b07001f96162f0c/scip_indexer/SCIPIndexer.cc#L749-L755) get `ReadAccess`. There is no relationship to tie the occurrence to. | signal on the occurrence, undocumented; nothing to confirm it against | The qualifiers also get role 0. In `class Fx::Qualified < Fx::Base` and `include Fx::Bar`, the `Fx`s are role 0 too, both the class name's scope (before the name) and the ancestor's. `extend` and `prepend` are not marked. |
| scip_dart | Position only. Ranges exist on classes, methods (annotations included) and type parameters (the bound included: `30:10-23`). A field's range covers only its name (`4:7-12`). | heuristic | `Bar? field` passes, since its type lies outside the field's range, and so does `Both`. The bound in `Bnd` is correctly rejected. |

A consequence that holds wherever the header test works: the occurrences that pass it and whose symbol is a relationship target are the **direct, written supertypes**. Comparing them with the relationships separates direct targets from inherited ones (section 4). This works only as well as the test does.

**Method relationships do not help (A).** scip-java, scip-typescript, scip-dotnet, scip-python, scip-go and scip_dart relate a method to the supertype method it implements (`is_implementation`, plus `is_reference` in all but scip-python and scip-go). scip-java also relates the interface method back to each implementation. These relate two definitions, not an occurrence, so they confirm that T implements X but do not say where X is written.

## 4. Facts: question 4, ancestry, externals and interface inheritance

| Indexer | Indirect ancestry (`A extends B`, `B implements C`) | Interface extends interface | External supertype | Other |
| --- | --- | --- | --- | --- |
| scip-java | **Transitive**: `A` → `B`, `C` | yes, `SubBar` → `Bar` | yes, the whole chain: `Ext extends AbstractList<String>` → `jdk java/util/AbstractList#`, `AbstractCollection#`, `Collection#`, `Iterable#`, `List#`, `SequencedCollection#` | It adds implicit supertypes: a record → `java/lang/Record#`, an enum → `Enum#`, `Constable#`, `Comparable#`, `Serializable#`, an annotation → `Annotation#`. Generic arguments are dropped. |
| scip-typescript | **Transitive** for classes, through heritage clauses | **none**: no type relationships for interfaces | yes, `extlib index.d.ts/ExtBase#` and `ExtIface#`. `extends Error` gives two targets, the declarations in `lib.es5.d.ts` and `lib.es2022.error.d.ts`. | `class ViaSub implements SubBar` → `SubBar#` and `Bar#` (through the interface). A structurally matching class gets nothing. |
| scip-dotnet | **Transitive**: `A` → `B`, `IC` | yes, `ISubBar` → `IBar` | yes, `System.Runtime System/Exception#`, plus `Serialization/ISerializable#`, which `Exception` implements | A record gets `System/IEquatable#`. A struct relates like a class. |
| scip-python | **Direct only**: `A` → `B` | yes, as class inheritance: `SubBar(Bar, ABC)` → `Bar#`, `abc/ABC#` | yes, `python-stdlib builtins/Exception#`, `abc/ABC#`, `typing/Generic#`, `typing/Protocol#` | The target can differ from the written symbol (`typing/Sized#` against `builtins/Sized#`, section 3). A `Protocol` matched structurally gets nothing. |
| scip-go | n/a (structural). Only concrete types are matched ([`implementations.go` L62-L123](https://github.com/scip-code/scip-go/blob/173cadc66acdda32e356cf7e02e00311363dfc07/internal/implementations/implementations.go#L62-L123), [`extractor.go` L145-L149](https://github.com/scip-code/scip-go/blob/173cadc66acdda32e356cf7e02e00311363dfc07/internal/implementations/extractor.go#L145-L149)). | **none**: an interface never gets type relationships | yes, against every interface of every package the project loads, imported by the file or not: `other.File` → `github.com/golang/go/src io/Closer#`, in a file that imports nothing | Generic interfaces get none (section 6). A promoted method shows up as a self-relationship `Bar#Copy.` → `Bar#Copy.` (section 6). |
| rust-analyzer | none | none (`trait SubBar: Bar`) | none | — |
| scip-php | none | none; the hover text says `interface SubBar implements \App\Bar` for `extends` ([`DocGenerator.php` L123-L128 at `71a5b117`](https://github.com/davidrjenni/scip-php/blob/71a5b117ec4c5dd2af302e363410e604e5df309e/src/DocGenerator.php#L123-L128)) | none | Supertypes appear only in the `documentation` markdown, as written: `class Foo extends App\Base implements \App\Bar\n\App\Repository`. |
| scip-ruby | none | n/a | none | The only relationships are `is_reference` for a constant alias and `is_definition` from a subclass's instance variable to the parent's (`Child#@greeting.` → `Parent#@greeting.`). A method override gets none. `StandardError` is declared as `scip-ruby gem project 0 StandardError#`, under the project's own package name. |
| scip_dart | **Transitive**, every supertype but `Object` | yes, `SubBar implements Bar` → `Bar#` | yes, `extpkg lib/extpkg.dart/ExtBase#`, `dart:core … Comparable#`, `Exception#` | Mixins applied with `with` are targets (`WithMixin` → `Foo`, `Base`, `Bar`, `Repository`, `Greets`). A mixin's own `on Bar` gets nothing. |

The transitive indexers give no flag that tells a written supertype from an inherited one. Removing the targets that an ancestor already has would also remove a supertype written again. In `class A extends B implements C`, where `B implements C`, the written `C` would be dropped.

## 5. Facts: question 2, Rust

There are no relationships. The open upstream issue is [rust-lang/rust-analyzer#14031](https://github.com/rust-lang/rust-analyzer/issues/14031), "Implement `Relationship` support for our SCIP backend", opened 2023-01-26.

| Case (file) | What the index shows |
| --- | --- |
| `impl Bar for Foo { fn copy… }` in `types.rs`, the type's module | The method is `fx types/impl#[Foo][Bar]copy().`, and the trait has a plain reference at 10:5. Nothing ties the reference to the method. The method's range (11:4–14:5) does not cover the `impl` line. |
| `impl Marker for Foo {}` in `impls.rs`, a third module | References to `contract/Marker#` and `types/Foo#` only. No `impl#…[Marker]` symbol exists anywhere in the index. |
| `impl Defaulted for Foo {}`, which uses only default methods | The same: two references and no symbol. |
| `impl Marker for crate::types::Order {}` in `contract.rs`, the trait's module | References to `Marker#` (with `trait Marker`'s own range, section 2), `crate/`, `types/` and `types/Order#`. There is no symbol. |
| `impl Bar for crate::types::Foo` in `other.rs`, for a second trait `other::Bar` | `fx other/impl#[`crate::types::Foo`][Bar]copy().` The descriptor's namespace is the **impl's module**. The type is the text as written, and the trait is the bare text `Bar`, the same as the impl of `contract::Bar` in `types.rs`. The descriptor cannot tell the two traits apart. |
| `impl WithType for Order { type Out = u8; }`, an associated type only | `other/impl#[`crate::types::Order`][WithType]Out#`, kind `AssociatedType`. An impl that defines any item leaves a symbol. |
| `impl Repository<Box<dyn Bar>> for Holder` | `impls/impl#[Holder][`Repository<Box<dyn Bar + 'static>>`]get().`: the generic argument survives as display text, and `Bar` as a type argument is an ordinary reference. |
| `impl Default for Holder { fn default() -> Self }` | `Self` is a reference to `impls/impl#[Holder][Default]`, the impl itself, with the impl block's range (36:0–40:1), which covers the trait on line 36. This is the only time the impl appears as a symbol, and only because `Self` is written. |
| `trait SubBar: Bar` | References only, no relationship. |
| `#[derive(Clone, Debug)]`, and the blanket `impl<T: Bar> Named for T {}` | No impl symbol. |

Where the evidence stands: wherever there is any, it is at the **impl site**, in the module that contains the impl, never at the type's definition. Impls that define no item have none at all. Tying an impl to its trait's symbol would mean resolving the descriptor's text (`Bar`, `` `crate::types::Foo` ``) in the impl's scope, which the index does not do.

## 6. Facts: question 3, Go

| Case | Relationships | Occurrences |
| --- | --- | --- |
| `*Foo` has `Copy() bar.Bar` | `Foo#` → `bar/Bar#`, and `Foo#Copy().` → `Bar#Copy.` | `bar.Bar` as a field type, return type, local and parameter: all `ReadAccess`, none distinguished |
| `var _ bar.Bar = (*Foo)(nil)` | none from the assertion. `Foo` → `Bar` comes from its method set alone. | two package-level `ReadAccess` references (`Bar#` and `Foo#`) |
| `var _ bar.Repository[bar.Order] = (*Foo)(nil)` and `GenImpl` with `Get() bar.Order` | **none**, to the generic interface, from either type, although both satisfy `Repository[Order]`. `types.Implements` is called with the uninstantiated interface ([L91-L92](https://github.com/scip-code/scip-go/blob/173cadc66acdda32e356cf7e02e00311363dfc07/internal/implementations/implementations.go#L91-L92)). | a reference to `Repository#` |
| `Stringer` with `String()` and `Write()`, never meant as either | `fmt/Stringer#` and `io/Writer#` | — |
| `other.File` with `Close() error`, in a file that imports nothing | `other/Closer#` (a local interface it never names) and **`io/Closer#`**: interfaces are taken from every package the project loads ([L125-L174](https://github.com/scip-code/scip-go/blob/173cadc66acdda32e356cf7e02e00311363dfc07/internal/implementations/implementations.go#L125-L174)) | — |
| `other.Cloner`, matching `other.Copier` in the same package | `Cloner#` → `Copier#` | — |
| `type SubBar interface { Bar; Extra() }` | none for `SubBar`. A struct with both methods gets `Bar#` and `SubBar#`. | `ReadAccess` reference to `Bar#`, no definition beside it |
| `type Embedded struct{ bar.Bar }` | `Embedded#` → `Bar#`. The promoted method's symbol is `Bar#Copy.` itself, so the method relationship lands on `Bar#Copy.` as `Bar#Copy.` → `Bar#Copy.`. | a reference to `Bar#` on the same range as the definition of the field `Embedded#Bar.` |

scip-go also emits "remote type implements local interface" as external symbols ([L159-L171](https://github.com/scip-code/scip-go/blob/173cadc66acdda32e356cf7e02e00311363dfc07/internal/implementations/implementations.go#L159-L171)). None arose in this fixture.

## 7. Facts: question 5, Dart

scip_dart 1.8.1 ran (Method). Compared with the claims gate-or-linter.md drew from its source:

- **Confirmed: transitive `is_implementation` to every supertype but `Object`.** `A` → `B`, `C`. `Marker implements SubBar` → `SubBar`, `Bar`. `WithMixin extends Foo with Greets` → `Foo`, `Base`, `Bar`, `Repository`, `Greets`. The source does this through `allSupertypes`, filtered with `!isDartCoreObject` ([`relationship_generator.dart` L14-L28 at `fef29da`](https://github.com/Workiva/scip-dart/blob/fef29da4c46efa3672056ef267ec32a7ea34f41a/lib/src/relationship_generator.dart#L14-L28)). Only a `ClassElement` gets type relationships, so the mixin `Greets on Bar` gets none (the comment at L11-L13 says so). Methods relate with `is_implementation` and `is_reference` (L32-L71).
- **Confirmed: kinds.** Every class, `abstract interface class` included, is `Class`, and the mixin is `Mixin`.
- **Confirmed: signature text** is the analyzer's display string, modifiers included: `abstract interface class Bar`, `abstract class Base`, `class Foo extends Base implements Bar, Repository<Order>`, `class Bnd<T extends Bar> implements Bar`, `mixin Greets on Bar`. No signature has `occurrences`.
- **Question 1:** this is in section 3. Position only, a heuristic: a field type is outside the field's range, and `Both` is ambiguous.

## 8. Facts: question 6, PHP, Ruby, Kotlin and Swift

- **scip-php at `71a5b117`** emits no relationships. Its source has no code that sets one: a search for "Relationship" finds only the generated bindings. That commit is the head of `main` ([commits](https://github.com/davidrjenni/scip-php/commits/main)). Upstream:
  - [#776](https://github.com/davidrjenni/scip-php/pull/776), "Enhance SCIP relationship data and type resolution", was closed unmerged on 2026-01-26. It proposed emitting relationships and telling extends (`is_reference`) from implements (`is_implementation`).
  - [#792](https://github.com/davidrjenni/scip-php/pull/792), which fills `enclosing_range` on definitions, is open.
  - [#513](https://github.com/davidrjenni/scip-php/pull/513), "Annotate overridden methods", was merged in 2024. It changes the parser, type internals and tests, and the observed `Foo#copy()` carries no relationship.
  - No open issue asks for relationships.
- **scip-ruby 0.5.1** emits no type relationships (section 4). Ruby's one marker is the role-0 reference on `<` and `include` (section 3). Upstream, [sourcegraph/scip-ruby#226](https://github.com/sourcegraph/scip-ruby/issues/226), "Emit implementation relationships for superclasses, mixins and ancestor methods", has been open since 2024-10-28, and [#114](https://github.com/sourcegraph/scip-ruby/issues/114) (method override relationships) is closed. The observed override in `Child#hello` had none.
- **Kotlin** ([`src/lang/kotlin.rs`](../../src/lang/kotlin.rs#L9)) runs one tree-sitter query, `(import) @node`, and resolves each import to a file by package and the names at the start of lines ([L47-L54](../../src/lang/kotlin.rs#L47-L54)). It reads no declaration, supertype or `: X` clause.
- **Swift** ([`src/lang/swift.rs`](../../src/lang/swift.rs#L9)) runs `(import_declaration) @node` and maps a module to `Sources/<module>` ([L23-L26](../../src/lang/swift.rs#L23-L26)). It reads no conformance.

## 9. What the evidence allows

These are conclusions from sections 1–8, not facts. They do not choose between the framings beyond what the evidence forces.

### Framing (A): a qualifier on the written occurrence

- **Reliable in no language.** No indexer marks the supertype occurrence in a way the spec defines, and no reference carries a parent-expression range.
- **Heuristic** in Java, TypeScript (classes only), Python and Dart, by the header test plus the relationship targets. The errors are known: X written both as a supertype and as a type argument of another supertype (`Both`), and in Java a bound when the type has a type parameter. Field types pass in TypeScript, Python and Dart. A Java initializer block passes. In Python an external base resolves to a different symbol than its relationship.
- **Ruby** has an occurrence-level marker, the role-0 ancestor reference, for `<` and `include` but not `extend` or `prepend`. Qualifiers are marked too, and it is undocumented behaviour of `saveQualifierReferences`. No relationship exists to confirm it.
- **No signal** in C# (no ranges), PHP (no relationships, no ranges), Rust (no relationships, and no occurrence for the impl block), Kotlin or Swift (the adapters read imports only).
- **Go** mostly has no written occurrence to qualify. Satisfaction is structural, and the assertion idiom is not tied to the type. Struct embedding is the one written case, with a co-located field definition (one observation).

Under (A) every misread occurrence is still a written name, so it stays a dependency. What a misread costs depends on a design choice the evidence does not settle. If a providing occurrence still counts for the dependency rules (`may_depend_on`), a misread changes only which rule judges it. If use rules skip providing occurrences, a misread hides a use. Evidence gaps (A) would have to report:

- "supertype occurrences not identified" for C#, PHP, Rust, Kotlin and Swift;
- "identified by position" for Java, TypeScript, Python and Dart, with the cases above;
- TypeScript interfaces, which have no relationship;
- Go, where implementation is not written.

### Framing (B): a separate graph of relationships

- **Served** by Java, C# and Dart (declared, transitive) and TypeScript (declared, transitive, classes only).
- **Served** by Python (declared, direct only).
- **Served** by Go (structural only, concrete types only, no generic interfaces). Go includes accidental satisfaction against any interface of any loaded package, the standard library's included.
- **Not served** by Rust (impls only as descriptor text, and nothing for item-less impls), PHP, Ruby, Kotlin or Swift.

Provenance is not in the index. Declared versus structural follows from the indexer alone: Go is structural, the others declared. Direct versus transitive differs by indexer, and no flag marks it. Recovering the direct set needs either the header test, which is (A)'s machinery with its errors, or a reduction that also drops a supertype written again. A relationship has no occurrence, so a finding could stand only at T's definition, never at the written clause. For Rust, a descriptor puts it at the impl site.

Evidence gaps (B) would have to report:

- languages with no relationships;
- TypeScript interfaces;
- Go generic interfaces and Go interface embedding;
- Rust item-less impls, and Rust trait identity, which is only text;
- the external symbol mismatch in Python.

(B)'s edges are not written names: transitive targets and Go's structural ones were never written by the file. So (B) sits outside [ADR 0003](../adr/0003-a-dependency-is-a-written-name.md)'s definition of a dependency, as the framing intends.

### Open questions

- Is a position heuristic with known misreadings acceptable in a gate whose output must say what not to trust (AGENTS.md)? That decides whether (A) can claim Java, TypeScript, Python and Dart.
- Should Rust's impl descriptors be read at all? Their trait and type are display text, resolved in the impl's scope, so this goes beyond reading the index, and item-less impls stay invisible whatever is decided.
- Is scip-ruby's role-0 ancestor reference stable enough to rely on, or should Ruby wait for scip-ruby#226?
- Kotlin and Swift have no facts either way. Either framing would need their adapters to read declarations, which AGENTS.md says needs a general mechanism agreed in an issue.
- **Unverified here:**
  - external supertypes from a third-party dependency package in Java, C#, Python, Go and Rust. Only TypeScript (`extlib`) and Dart (`extpkg`) tried one; the others tried only the platform or standard library (JDK, BCL, Python stdlib, Go std, Rust `core`);
  - Java on Gradle;
  - TypeScript class expressions and mixin functions were not run. The source read excludes them from type relationships (section 3);
  - C# partial classes;
  - Java `default` methods and Rust default methods as bodies inside the header test;
  - whether easement's own runs give the same indexes. The commands were run directly, not through `easement` with `EASEMENT_CACHE_DIR`.
- These observations hold for the pinned versions. Transitivity and the role-0 marker are indexer behaviour, not spec, and may change with an upgrade.
