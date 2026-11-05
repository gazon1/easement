---
status: accepted
---

# A dependency is a name the code writes, in every language

`may_depend_on` meant two things. In PHP, Java, C# and Ruby a member use (a method call, a constructor) made a dependency on its class, so a method called on a value of inferred type counted. In Rust, TypeScript, Python and Go only a module reference made one, so a re-exported name or an inferred member did not ([#104](https://github.com/mokevnin/easement/issues/104)). The scheme checks in the core loop that caused this are gone. A dependency is now defined once: a name a file writes that is defined elsewhere, meaning a module or a name at the top level of one, never a member reached through a value. Every other reference is a symbol use, which the symbol rules read and the dependency rules do not. We chose this because what an indexer resolves for a written name is reliable in every language, while what it infers for a member is not: scip-ruby reports a call only where Sorbet knows the receiver's type, and scip-python types partially. Counting members would give one policy a different completeness in each language.

## Considered options

- **Every symbol use is a dependency, members included** (ArchUnit's model). This was rejected because the verdict would depend on how much each indexer infers, and because inferred, inherited and promoted uses would count: a method called on a value another component returned, or inherited through a third component's subclass.
- **Keep the per-language readings and state them in the registry and the report.** This was rejected because it names the inconsistency instead of removing it, and it contradicts "a rule is defined once for every language".

## Consequences

- A re-exported name (Rust `pub use`, a TypeScript barrel, a Python `__init__` import) is a dependency on the component that defines it, because the index points the use at the original. The re-export line as `--diff` evidence and alias following are left to [#103](https://github.com/mokevnin/easement/issues/103).
- An alias that is its own definition hides the original until #103: Go `type Block = blob.Block`, Ruby `Block = Blob::Block`, Rust `pub type`, TypeScript `export type`, a Python assignment.
- In Java, PHP, C# and Ruby a method called on a value whose type the file never writes is no longer a dependency. It is a symbol use, and whether symbol rules judge it in those languages is up to the registry.
- Some indexers record a written class name only as the class's constructor: scip-java for `new X()`, scip-dotnet for an attribute `[X]`. Reading the index, a constructor is read as its class. It is the only member treated this way. Ruby's `initialize` is not: its occurrence stands on `new`, and the class's own symbol is already beside it.
- No compatibility mode: this ships as a breaking change.
