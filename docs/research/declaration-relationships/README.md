# Evidence for declaration-relationships.md

The indexes behind [`../declaration-relationships.md`](../declaration-relationships.md), observed on 2026-11-08. One directory per indexer:

- `snapshot/**/*.snap`: the output of `scip snapshot --strict=false` on the fixture's own files, each occurrence and its `SymbolInformation` (kind, signature, relationships, enclosing ranges) printed beside the source line. This is the primary evidence. `external_symbols.txt.snap` lists the external symbols the index declares.
- `decoded.txt`: the same index read by [`decode.py.txt`](decode.py.txt) over `scip print --json`. It prints every `SymbolInformation` that has relationships (`REL`), every type definition with its enclosing range (`DEF`), with `--alldefs` every definition (`ALLDEF`), and every occurrence of the chosen symbols (`OCC`) with its range, roles, `syntax_kind` and enclosing range.

The fixture projects themselves are not kept here. With them in place, easement's own check (`cargo run -- check`, `easement.yml`) failed at the first one it met: "dependencies are not installed in docs/research/declaration-relationships/php/fixture: run `composer install`". The other manifests (`pom.xml`, `go.mod`, `Cargo.toml`, …) were not tried one by one. The decoder is kept as `.txt` as a precaution, so that the check cannot take it for Python source (not tried as `.py`). The sources are recoverable from the snapshots, and the manifests are below.

## The header test in `decoded.txt`

`HEADER(T; rel-target)` on an occurrence is the decoder's own heuristic, not something any indexer emits: the occurrence lies inside the enclosing range of the definition of type `T`, starts after `T`'s name, and lies inside no enclosing range of another definition within `T` (a definition whose range equals `T`'s, as scip-java gives a type parameter, is ignored). `rel-target` means the occurrence's symbol is one of `T`'s relationship targets. It is the best position-only test the ranges allow, and the note records where it errs.

## Recovering a fixture

Every source line of a snapshot is indented by two spaces; every annotation line starts with `//` in column 0. For each `X.snap`:

```sh
grep -v '^//' X.snap | sed 's/^  //' > X
```

Go fixtures were `gofmt`ed and the snapshot shows tabs as one space: run `gofmt -w .` afterwards (checked: byte-identical). Then add the manifests below and run the indexer as easement runs it ([`INDEXERS`](../../../src/indexer.rs#L181)), in the fixture's root:

| Directory | Prepare | Index |
| --- | --- | --- |
| `java` | | `scip-java index --output index.scip` |
| `ts` | `mkdir -p node_modules/extlib`, then the two `extlib` files below | `scip-typescript index --no-progress-bar --output index.scip` |
| `csharp` | | `scip-dotnet index --output index.scip` |
| `python` | | `scip-python index . --quiet --project-name project --project-version 0 --output index.scip` |
| `go` | | `scip-go --output index.scip` |
| `rust` | | `rust-analyzer scip . --output index.scip` |
| `php` | `composer install` (scip-php reads `vendor/composer/installed.php`) | `scip-php` (writes `index.scip` in the current directory) |
| `ruby` | | `scip-ruby --gem-metadata project@0 --index-file index.scip .` |
| `dart` | `dart pub get`; the `extpkg` package below | see "Dart" |

Then `scip snapshot --strict=false --from index.scip --to <out>` from the same directory (it reads the sources relative to the current directory), and `scip print --json index.scip > i.json && python3 -I decode.py i.json . <symbol substrings…>` with these substrings:

| Directory | Substrings |
| --- | --- |
| `java` | ``'fx/Bar#' 'fx/SubBar#' 'fx/C#' 'fx/Repository#' --alldefs`` |
| `ts` | ``'src/`bar.ts`/Bar#' 'SubBar#' 'chain.ts`/C#' 'Repository#' 'ExtBase#' 'ExtIface#' 'Error#' 'BarAlias#' --alldefs`` |
| `csharp` | ``'Fx/IBar#' 'Fx/ISubBar#' 'Fx/IC#' 'IRepository#' 'Exception#' 'IDisposable#' 'IComparer#' --alldefs`` |
| `python` | ``'`fx.bar`/Bar#' '`fx.bar`/SubBar#' 'chain`/C#' 'Repository#' 'Exception#' 'Sized#' 'Copyable#' --alldefs`` |
| `go` | ``'bar`/Bar#' 'bar`/SubBar#' 'Repository#' 'Stringer#' 'Writer#' 'Copier#' 'Closer#' --alldefs`` |
| `rust` | ``'contract/Bar#' 'other/Bar#' 'Marker#' 'Defaulted#' 'SubBar#' 'Repository#' 'Display#' 'Default#' 'Named#' 'WithType#' 'impl#' --alldefs`` |
| `php` | ``'App/Bar#' 'App/SubBar#' 'App/C#' 'App/B#' 'App/Base#' 'Greets#' 'Exception#' 'Countable#' 'App/Repository#'`` |
| `ruby` | ``'Fx#Bar#' 'Fx#Base#' 'Fx#C#' 'Fx#B#' 'Fx#Parent#' 'StandardError#' 'Comparable#' 'Alias' 'Fx#' --alldefs`` |
| `dart` | ``'bar.dart`/Bar#' 'SubBar#' 'chain.dart`/C#' 'Repository#' 'ExtBase#' 'Comparable#' 'Exception#' 'Greets#' --alldefs`` |

Versions: the ones `mise.toml` pins (scip-java 0.13.2, scip-typescript 0.6.1, scip-python 0.6.8, scip-go 0.2.9, scip-ruby `scip-ruby-v0.5.1`, scip 0.10.0, `rust` 1.99.1 for rust-analyzer), scip-dotnet 0.2.16 and scip-php at `71a5b117` as the CI workflow installs them.

### Dart

scip_dart 1.8.1 as PR #71 pins it ([`tools/scip-dart/pubspec.yaml` at `59576d4`](https://github.com/Bayrampro/easement/blob/59576d43c93c8a2712276a8a357905b35c2b99bf/tools/scip-dart/pubspec.yaml)), run with the Dart SDK 3.13.7 installed ad hoc (`mise exec dart@3.13.7 -- …`, not in `mise.toml`):

```sh
# a directory with PR #71's tools/scip-dart/pubspec.yaml and pubspec.lock
dart pub get
# in the fixture, the command PR #71's launcher (tools/scip-dart/bin/install.dart) writes,
# with the arguments its indexer entry passes (`--output <file> .`)
dart --packages=<that directory>/.dart_tool/package_config.json \
  ~/.pub-cache/hosted/pub.dev/scip_dart-1.8.1/bin/scip_dart.dart --output index.scip .
```

## Manifests

`java/pom.xml`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 http://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>fx</groupId>
  <artifactId>fx</artifactId>
  <version>0</version>
  <properties>
    <maven.compiler.release>21</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
</project>
```

`ts/package.json`, `ts/tsconfig.json`, `ts/node_modules/extlib/package.json`, `ts/node_modules/extlib/index.d.ts`:

```json
{ "name": "fx", "version": "0.0.0", "private": true, "dependencies": { "extlib": "1.0.0" } }
```

```json
{ "compilerOptions": { "target": "es2022", "module": "nodenext", "strict": true, "noEmit": true }, "include": ["src"] }
```

```json
{ "name": "extlib", "version": "1.0.0", "types": "index.d.ts" }
```

```ts
export declare class ExtBase { name(): string; }
export interface ExtIface { run(): void; }
```

`csharp/fx.csproj`:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <OutputType>Library</OutputType>
    <Nullable>enable</Nullable>
  </PropertyGroup>
</Project>
```

`python/pyproject.toml` (and an empty `fx/__init__.py`):

```toml
[project]
name = "fx"
version = "0"
```

`go/go.mod`:

```text
module example.com/fx

go 1.23
```

`rust/Cargo.toml`:

```toml
[package]
name = "fx"
version = "0.0.0"
edition = "2021"

[workspace]
```

`php/composer.json` (classmap autoloading: scip-php's PSR-4 scan only finds a class in a file of its name):

```json
{
  "name": "fx/fx",
  "autoload": { "classmap": ["src/"] },
  "require": {}
}
```

`ruby/Gemfile`:

```ruby
source "https://rubygems.org"
```

`dart/pubspec.yaml`, `dart/extpkg/pubspec.yaml`, `dart/extpkg/lib/extpkg.dart`:

```yaml
name: fx
version: 0.0.0
publish_to: none
environment:
  sdk: ">=3.6.0 <4.0.0"
dependencies:
  extpkg:
    path: extpkg
```

```yaml
name: extpkg
version: 1.0.0
environment:
  sdk: ">=3.6.0 <4.0.0"
```

```dart
abstract class ExtBase {
  String name();
}
```
