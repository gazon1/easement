//! Reads a SCIP index and turns it into the facts the rules check.
//!
//! An indexer (`scip-go`, `scip-typescript`, …) resolves every name in the
//! project the way the compiler does. Two kinds of references matter here:
//!
//! - a reference to a *module* (a Go package, a TypeScript file, a Java class) stands
//!   on the lines that import or use that module, so it is a dependency;
//! - a reference to a name at the top level of a module (`db.Client`) is a dependency
//!   on the file that defines it, also through a re-export;
//! - a reference to a *member* reached through a value (`c.Query()`) is only a use of
//!   a name.
//!
//! Every scheme is read into one shape first (see [`Name`]), so the facts are made
//! the same way for every language.
//!
//! References to symbols that no document of the index defines point outside
//! the project: they are external modules.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use protobuf::Message;
use scip::types::{Index, Occurrence, PositionEncoding, occurrence::Typed_range};

use crate::check::SymbolRef;
use crate::flow::{Edge, Flow, Node};
use crate::lang::parts::Part;
use crate::lang::registry::Manifest;
use crate::lang::{Dep, Scope, Written};
use crate::lang::{Target, Unit};

/// `SymbolRole::Definition` in the bitset of an occurrence.
const DEFINITION: i32 = 1;

/// One reference to a symbol in a document.
#[derive(Debug, Clone)]
pub struct Reference {
    /// 1-based line.
    pub line: usize,
    pub symbol: String,
    /// For a reference into an unnamed npm package (see `UNNAMED_NPM`) that
    /// stands on the string of an import: the module the string spells.
    pub spelled: Option<String>,
}

/// How scip-typescript begins a symbol defined under a `package.json` with no
/// `name`, such as the `{"type": "module"}` that marks `esm/` in a published
/// package: which package that is, only the import in the source still says.
const UNNAMED_NPM: &str = "scip-typescript npm . . ";

/// Why the package of a name under [`UNNAMED_NPM`] is unknown, for the report.
pub const UNNAMED: crate::check::Unnamed = crate::check::Unnamed {
    what: "the npm package",
    why: "a `package.json` without `name` inside the package hides it from the indexer, so those uses were not checked; scip-typescript 0.6.1 or newer names such a package: update the indexer and run again",
};

/// A source file of the index, with the paths relative to the project root.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub path: String,
    pub references: Vec<Reference>,
    /// Symbols this file defines.
    pub definitions: Vec<String>,
    /// The types this file defines (classes, structs, interfaces, traits),
    /// by name and 1-based line.
    pub types: Vec<(String, usize)>,
    /// The methods and functions this file defines that the index gave a range.
    pub scopes: Vec<Scope>,
    /// The parts of the file that a dependency can belong to instead of the file.
    pub parts: Vec<crate::lang::parts::Span>,
}

/// The first and last line, 0-based, of the enclosing range of an occurrence.
fn enclosing_lines(occurrence: &Occurrence) -> Option<(usize, usize)> {
    use scip::types::occurrence::Typed_enclosing_range as Typed;
    let (first, last) = match (
        &occurrence.typed_enclosing_range,
        occurrence.enclosing_range.as_slice(),
    ) {
        (Some(Typed::SingleLineEnclosingRange(range)), _) => (range.line, range.line),
        (Some(Typed::MultiLineEnclosingRange(range)), _) => (range.start_line, range.end_line),
        (Some(_), _) => return None,
        (None, [line, _, _]) => (*line, *line),
        (None, [start, _, end, _]) => (*start, *end),
        (None, _) => return None,
    };
    Some((usize::try_from(first).ok()?, usize::try_from(last).ok()?))
}

/// The documents of one or more indexes, merged.
#[derive(Debug, Default)]
pub struct Documents(pub Vec<Document>);

/// Parses an index written by an indexer that ran in `base` (a directory
/// relative to the project `root`, `""` for the root itself). Documents outside
/// the project, such as generated files in a build cache, are dropped. The
/// source of a document is read only for a reference into an unnamed package.
pub fn parse(bytes: &[u8], root: &Path, base: &Path) -> Result<Vec<Document>> {
    let index = Index::parse_from_bytes(bytes).context("not a SCIP index")?;
    let prefix = crate::relpath::slash(base);
    let mut documents = Vec::new();
    for document in &index.documents {
        // Indexers on Windows may write `\` separators.
        let normalized = document.relative_path.replace('\\', "/");
        let relative = normalized.as_str();
        if relative.starts_with("..")
            || relative.starts_with('/')
            || relative.as_bytes().get(1) == Some(&b':')
        {
            continue;
        }
        let path = match prefix.as_str() {
            "" => relative.to_string(),
            base => format!("{base}/{relative}"),
        };
        let encoding = document.position_encoding.enum_value_or_default();
        let mut source: Option<Vec<String>> = None;
        let mut parsed = Document {
            path,
            ..Document::default()
        };
        for occurrence in &document.occurrences {
            if occurrence.symbol.is_empty() || occurrence.symbol.starts_with("local ") {
                continue;
            }
            if occurrence.symbol_roles & DEFINITION != 0 {
                if let Some(parsed_symbol) = split(&occurrence.symbol)
                    && let Some((name, Kind::Type)) = parsed_symbol.descriptors.last()
                {
                    parsed.types.push((name.clone(), line_of(occurrence) + 1));
                }
                // A method or function is `name().`; a parameter is `(name)`.
                if occurrence.symbol.ends_with(").")
                    && let Some((first, last)) = enclosing_lines(occurrence)
                {
                    parsed.scopes.push(Scope {
                        symbol: occurrence.symbol.clone(),
                        first: first + 1,
                        last: last + 1,
                    });
                }
                parsed.definitions.push(occurrence.symbol.clone());
            } else {
                let spelled = match occurrence.symbol.starts_with(UNNAMED_NPM) {
                    true => {
                        let lines = source.get_or_insert_with(|| {
                            std::fs::read_to_string(root.join(&parsed.path))
                                .unwrap_or_default()
                                .lines()
                                .map(str::to_string)
                                .collect()
                        });
                        spelled_module(lines, occurrence, encoding)
                            .filter(|module| installed(&root.join(&parsed.path), module))
                    }
                    false => None,
                };
                parsed.references.push(Reference {
                    line: line_of(occurrence) + 1,
                    symbol: occurrence.symbol.clone(),
                    spelled,
                });
            }
        }
        documents.push(parsed);
    }
    Ok(documents)
}

/// Whether the npm package of `module` is in a `node_modules` above `file`,
/// where TypeScript finds a package. A project without `name` in its
/// `package.json` is unnamed too, and an alias into it (`@/gen/api` through
/// `paths`) spells no package.
fn installed(file: &Path, module: &str) -> bool {
    let package = Manifest::Npm.package(module);
    file.ancestors()
        .skip(1)
        .any(|dir| dir.join("node_modules").join(&package).is_dir())
}

/// The package an import names when `occurrence` stands on its string
/// (`"hono"`, `'@scope/pkg/sub'`); `None` for a name, a relative path or a
/// range over more than one line.
fn spelled_module(
    lines: &[String],
    occurrence: &Occurrence,
    encoding: PositionEncoding,
) -> Option<String> {
    let (line, start, end) = match (&occurrence.typed_range, occurrence.range.as_slice()) {
        (Some(Typed_range::SingleLineRange(r)), _) => (r.line, r.start_character, r.end_character),
        (Some(_), _) => return None,
        (None, [line, start, end]) => (*line, *start, *end),
        (None, _) => return None,
    };
    let text = lines.get(usize::try_from(line).ok()?)?;
    // The byte offset of a column, counted in the code units of the index;
    // TypeScript counts UTF-16 ones when the index does not say.
    let offset = |column: i32| -> Option<usize> {
        let column = usize::try_from(column).ok()?;
        let mut units = 0;
        for (byte, c) in text.char_indices() {
            if units == column {
                return Some(byte);
            }
            units += match encoding {
                PositionEncoding::UTF8CodeUnitOffsetFromLineStart => c.len_utf8(),
                PositionEncoding::UTF32CodeUnitOffsetFromLineStart => 1,
                _ => c.len_utf16(),
            };
        }
        (units == column).then_some(text.len())
    };
    let quoted = text.get(offset(start)?..offset(end)?)?;
    let module = ['"', '\'', '`']
        .into_iter()
        .find_map(|quote| quoted.strip_prefix(quote)?.strip_suffix(quote))?;
    let bare = !module.is_empty()
        && !module.starts_with(['.', '/'])
        && !module.contains("${")
        && !module.contains(['"', '\'', '`']);
    bare.then(|| module.to_string())
}

/// The start line, 0-based, from the typed range or the deprecated one.
fn line_of(occurrence: &Occurrence) -> usize {
    let line = match &occurrence.typed_range {
        Some(Typed_range::SingleLineRange(range)) => range.line,
        Some(Typed_range::MultiLineRange(range)) => range.start_line,
        Some(_) | None => occurrence.range.first().copied().unwrap_or(0),
    };
    usize::try_from(line).unwrap_or(0)
}

/// How a descriptor of a symbol ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Namespace,
    /// A class, interface or other type: `Name#`.
    Type,
    Other,
}

/// A symbol split into the package that owns it and its descriptors.
#[derive(Debug)]
struct Parsed<'a> {
    scheme: &'a str,
    package: &'a str,
    descriptors: Vec<(String, Kind)>,
}

/// `scheme manager package version descriptors`.
fn split(symbol: &str) -> Option<Parsed<'_>> {
    let mut parts = symbol.splitn(5, ' ');
    let scheme = parts.next()?;
    let _manager = parts.next()?;
    let package = parts.next()?;
    let _version = parts.next()?;
    let descriptors = descriptors(parts.next()?)?;
    Some(Parsed {
        scheme,
        package,
        descriptors,
    })
}

/// The descriptors of a symbol: a name, quoted in backticks when it holds
/// anything but identifier characters, and a suffix (`/` namespace, `#` type,
/// `.` term, `().` method, `:` meta, `!` macro, `[T]` type parameter, `(p)` parameter).
fn descriptors(text: &str) -> Option<Vec<(String, Kind)>> {
    scan(text).map(|found| {
        found
            .into_iter()
            .map(|(name, kind, _)| (name, kind))
            .collect()
    })
}

/// The descriptors with the offset, in characters, at which each one ends.
fn scan(text: &str) -> Option<Vec<(String, Kind, usize)>> {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    let mut found = Vec::new();
    while at < chars.len() {
        let mut name = String::new();
        if chars[at] == '`' {
            at += 1;
            loop {
                match chars.get(at)? {
                    '`' if chars.get(at + 1) == Some(&'`') => {
                        name.push('`');
                        at += 2;
                    }
                    '`' => {
                        at += 1;
                        break;
                    }
                    c => {
                        name.push(*c);
                        at += 1;
                    }
                }
            }
        } else if chars[at] == '[' || chars[at] == '(' {
            // A type or value parameter descriptor starts with its bracket.
        } else {
            while let Some(c) = chars.get(at)
                && (c.is_alphanumeric() || matches!(c, '_' | '+' | '-' | '$'))
            {
                name.push(*c);
                at += 1;
            }
        }
        let kind = match chars.get(at)? {
            '/' => {
                at += 1;
                Kind::Namespace
            }
            '#' => {
                at += 1;
                Kind::Type
            }
            '.' | ':' | '!' => {
                at += 1;
                Kind::Other
            }
            '(' => {
                // `name().` or `name(disambiguator).`; a parameter is `(name)`.
                let close = chars[at..].iter().position(|c| *c == ')')? + at;
                at = close + 1;
                if chars.get(at) == Some(&'.') {
                    at += 1;
                }
                Kind::Other
            }
            '[' => {
                let close = chars[at..].iter().position(|c| *c == ']')? + at;
                if name.is_empty() {
                    name = chars[at + 1..close].iter().collect();
                }
                at = close + 1;
                Kind::Other
            }
            _ => return None,
        };
        found.push((name, kind, at));
    }
    Some(found)
}

/// `symbol` with only its first `count` descriptors.
fn cut(symbol: &str, count: usize) -> String {
    let mut parts = symbol.splitn(5, ' ');
    let header: Vec<&str> = parts.by_ref().take(4).collect();
    let text = parts.next().unwrap_or_default();
    let end = match count.checked_sub(1) {
        None => 0,
        Some(last) => scan(text)
            .and_then(|found| found.get(last).map(|(_, _, end)| *end))
            .unwrap_or_else(|| text.chars().count()),
    };
    format!(
        "{} {}",
        header.join(" "),
        text.chars().take(end).collect::<String>()
    )
}

/// What a symbol names, read the same way in every language. [`read`] turns the
/// symbol of each scheme into this shape; past it, nothing tells one language from
/// another, and [`facts_watching`] makes the same facts of the same shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Name {
    shape: Shape,
    /// The symbol whose definition is the name's home: the symbol itself, or the
    /// module that a constructor or a nested type stands for.
    symbol: String,
    /// For a member, the name it belongs to, whose home stands for the member's
    /// when the index defines no member.
    owner: Option<String>,
    /// The package that owns the symbol, with the scheme: what tells the project's own
    /// symbols from those of its dependencies. `None` when the index gives both the same.
    package: Option<String>,
    /// The names of the module the symbol is, or lives in.
    path: Vec<String>,
    /// The names below that module: the top-level name, then the member.
    inner: Vec<String>,
    /// The external module, in the spelling of the language's imports and of `forbid_external`.
    external: String,
    /// What `forbid_symbol` calls it; `None` for a module that is a place, not a name
    /// (a Go package, a TypeScript file).
    called: Option<Called>,
    reading: Reading,
}

/// What a name is to the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// A package or namespace named on its own, which is no dependency.
    Package,
    /// The unit names live in, a dependency even when it is external: a Go package, a
    /// TypeScript file, a Python or Rust module, a Java, C# or PHP class, a Ruby constant.
    Module,
    /// A name at the top level of a module (a class, a function, a constant): a
    /// dependency on the file that defines it. An external one is reached through its module.
    TopLevel,
    /// A member reached through a value (a method, a field): a symbol use only.
    Member,
}

/// How `forbid_symbol` and `public_symbols` call a name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Called {
    qualifier: Qualifier,
    name: String,
    /// The member of the name that is used: a method, a variant, a constant.
    member: Option<Member>,
}

/// A member of a name, and what `forbid_symbol` calls it through.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Member {
    through: Through,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Through {
    /// As the index spells it: `Shop::Db::Client`.
    Spelled(String),
    /// The qualifier of the name and the name, joined by this: `db::client` `::`
    /// `Client`, `app/db` `.` `Client`.
    Joined(&'static str),
}

impl Called {
    /// The member used, as `forbid_symbol` names it: what it is called through, and
    /// its name. `specifier` is the qualifier of the name as the use spells it.
    fn method(&self, specifier: &str) -> Option<(String, String)> {
        let member = self.member.as_ref()?;
        let through = match &member.through {
            Through::Spelled(owner) => owner.clone(),
            Through::Joined(separator) => format!("{specifier}{separator}{}", self.name),
        };
        Some((through, member.name.clone()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Qualifier {
    Spelled(String),
    /// The path of the file that defines the name, without its extension, or this
    /// when no file of the project does.
    File(String),
}

/// What the index of a language says of its symbols besides their shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Reading {
    /// A line that writes a path also references each module around it
    /// (`crate::domain::order`, `Shop::Domain::Order`): the line depends on the deepest.
    enclosing: bool,
    /// A definition names its module by its path from the working directory
    /// (`src.shop.infra`) and a reference as imported (`shop.infra`), so a reference is
    /// matched by the ending of its path.
    by_ending: bool,
    /// The index does not say which package the symbol is in; only the string of
    /// the import does (see `UNNAMED_NPM`).
    unnamed: bool,
}

/// `symbol` in the shape every language shares.
fn read(symbol: &str) -> Option<Name> {
    let parsed = split(symbol)?;
    Some(match parsed.scheme {
        "scip-ruby" => ruby(symbol, &parsed),
        // scip-java gives `new X()` only as the constructor of `X`.
        "scip-java" => classes(symbol, &parsed, ".", Some("<init>"), Some(parsed.owner())),
        // scip-dotnet gives an attribute `[X]` only as the constructor of `XAttribute`, and
        // the project's own types and the framework's the same header.
        "scip-dotnet" => classes(symbol, &parsed, ".", Some(".ctor"), None),
        "scip-php" => classes(symbol, &parsed, "\\", None, Some(parsed.owner())),
        _ => modules(symbol, &parsed),
    })
}

/// The names of the descriptors.
fn names(parsed: &Parsed) -> Vec<String> {
    parsed
        .descriptors
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}

/// How many descriptors from `from` on are of `kind`.
fn leading(parsed: &Parsed, from: usize, kind: Kind) -> usize {
    parsed.descriptors[from..]
        .iter()
        .take_while(|(_, k)| *k == kind)
        .count()
}

/// Go, TypeScript, Python and Rust: modules are namespaces, and a name at their top
/// level is the first descriptor past them.
fn modules(symbol: &str, parsed: &Parsed) -> Name {
    let names = names(parsed);
    let modules = leading(parsed, 0, Kind::Namespace);
    let mut path = names[..modules].to_vec();
    let mut inner = names[modules..].to_vec();
    let mut reading = Reading::default();
    let qualifier = match parsed.scheme {
        "scip-go" => Qualifier::Spelled(parsed.namespace("/")),
        "scip-python" => {
            // A module is the term `__init__` of its namespace.
            if modules == 1 && inner == ["__init__"] {
                inner.clear();
            }
            path = path
                .iter()
                .flat_map(|name| name.split('.'))
                .map(str::to_string)
                .collect();
            reading.by_ending = true;
            Qualifier::Spelled(parsed.namespace("."))
        }
        "rust-analyzer" => {
            // `crate/` is the root of the crate, around its modules (`domain/order/`).
            path.retain(|name| name != "crate");
            reading.enclosing = true;
            Qualifier::Spelled(parsed.external_module())
        }
        _ => {
            reading.unnamed = symbol.starts_with(UNNAMED_NPM);
            Qualifier::File(npm_name(parsed.package))
        }
    };
    let shape = match inner.len() {
        0 => Shape::Module,
        1 => Shape::TopLevel,
        _ => Shape::Member,
    };
    Name {
        shape,
        symbol: symbol.to_string(),
        owner: (shape == Shape::Member).then(|| cut(symbol, modules + 1)),
        package: Some(parsed.owner()),
        called: inner.first().map(|name| Called {
            qualifier,
            name: name.clone(),
            member: inner.get(1).map(|member| Member {
                // A Rust path joins a type and what is in it as it joins modules.
                through: Through::Joined(if parsed.scheme == "rust-analyzer" {
                    "::"
                } else {
                    "."
                }),
                name: member.clone(),
            }),
        }),
        external: parsed.external_module(),
        path,
        inner,
        reading,
    }
}

/// Java, C# and PHP: a namespace is no module; a class is what an import names, and
/// a type nested in it is reached through it. `constructor` is the member that stands
/// for its class: the indexer records the class the code writes only as that member.
fn classes(
    symbol: &str,
    parsed: &Parsed,
    separator: &str,
    constructor: Option<&str>,
    package: Option<String>,
) -> Name {
    let names = names(parsed);
    let namespaces = leading(parsed, 0, Kind::Namespace);
    let types = leading(parsed, namespaces, Kind::Type);
    let namespace = names[..namespaces].join(separator);
    let below = &names[namespaces + types..];
    let shape = match (types, below) {
        _ if namespaces == names.len() => Shape::Package,
        (0, [_]) => Shape::TopLevel,
        (0, _) => Shape::Member,
        (_, []) => Shape::Module,
        (_, [member]) if Some(member.as_str()) == constructor => Shape::Module,
        _ => Shape::Member,
    };
    let class = match types {
        0 => symbol.to_string(),
        _ => cut(symbol, namespaces + 1),
    };
    let path = names[..(namespaces + 1).min(names.len())].to_vec();
    Name {
        shape,
        symbol: match shape {
            Shape::Module => class.clone(),
            _ => symbol.to_string(),
        },
        owner: (shape == Shape::Member).then_some(class),
        package,
        called: (shape != Shape::Package).then(|| Called {
            qualifier: Qualifier::Spelled(namespace),
            name: names[namespaces].clone(),
            member: None,
        }),
        external: path.join(separator),
        inner: match shape {
            Shape::Module => Vec::new(),
            _ => names[(namespaces + 1).min(names.len())..].to_vec(),
        },
        path,
        reading: Reading::default(),
    }
}

/// Ruby: modules and classes are both types, and a constant names one; everything
/// below it is a member (`Shop#Web#Order#run().`). A class method belongs to the
/// singleton class (``Shop#Web#`<Class:Order>`#find().``), which stands for the class.
fn ruby(symbol: &str, parsed: &Parsed) -> Name {
    let names = names(parsed);
    let types = leading(parsed, 0, Kind::Type);
    let mut path = names[..types].to_vec();
    let mut class = cut(symbol, types);
    if let Some(last) = path.last_mut()
        && let Some(singleton) = last
            .strip_prefix("<Class:")
            .and_then(|name| name.strip_suffix('>'))
            .map(str::to_string)
    {
        class = format!("{}{singleton}#", cut(symbol, types - 1));
        *last = singleton;
    }
    let inner = names[types..].to_vec();
    let shape = match (types, inner.is_empty()) {
        (0, _) => Shape::Package,
        (_, true) => Shape::Module,
        (_, false) => Shape::Member,
    };
    let qualified = path.join("::");
    Name {
        shape,
        symbol: match shape {
            Shape::Module => class.clone(),
            _ => symbol.to_string(),
        },
        owner: (shape == Shape::Member).then_some(class),
        package: Some(parsed.owner()),
        called: path.split_last().map(|(name, outer)| Called {
            qualifier: Qualifier::Spelled(outer.join("::")),
            name: name.clone(),
            member: inner.first().map(|member| Member {
                through: Through::Spelled(qualified.clone()),
                name: member.clone(),
            }),
        }),
        external: qualified,
        path,
        inner,
        reading: Reading {
            enclosing: true,
            ..Reading::default()
        },
    }
}

impl Parsed<'_> {
    /// The namespace descriptors joined: the import path of a Go package, the
    /// dotted name of a Python module, the path of a Rust module.
    fn namespace(&self, separator: &str) -> String {
        self.descriptors
            .iter()
            .take_while(|(_, kind)| *kind == Kind::Namespace)
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(separator)
    }

    /// The package that owns the symbol, with the scheme: what tells the
    /// project's own symbols from those of its dependencies.
    fn owner(&self) -> String {
        format!("{} {}", self.scheme, self.package)
    }

    /// The external module the symbol of a language whose modules are namespaces
    /// belongs to, in the spelling of its import statements and of `forbid_external`.
    fn external_module(&self) -> String {
        match self.scheme {
            // Go: the package path (`fmt`, `net/http`, `github.com/lib/pq`).
            "scip-go" => self.namespace("/"),
            // Python: the dotted module (`os.path`, `requests`).
            "scip-python" => self.namespace("."),
            // Rust: the crate and the module path; the standard library is
            // spread over `core`, `alloc` and `std` but written `std::…`.
            "rust-analyzer" => {
                let krate = match self.package {
                    "core" | "alloc" | "std" => "std".to_string(),
                    other => other.replace('-', "_"),
                };
                std::iter::once(krate)
                    .chain(
                        self.descriptors
                            .iter()
                            .take_while(|(_, kind)| *kind == Kind::Namespace)
                            .map(|(name, _)| name.clone())
                            .filter(|name| name != "crate"),
                    )
                    .collect::<Vec<_>>()
                    .join("::")
            }
            // TypeScript: the npm package and the file inside it that was imported
            // (`lodash/fp`, `moment/locale/ru`); `@types/x` stands for `x`.
            _ => {
                let mut parts: Vec<String> = self
                    .descriptors
                    .iter()
                    .take_while(|(_, kind)| *kind == Kind::Namespace)
                    .map(|(name, _)| name.clone())
                    .collect();
                if let Some(last) = parts.last_mut() {
                    for suffix in [".d.ts", ".d.mts", ".d.cts", ".ts", ".tsx", ".js", ".mjs"] {
                        if let Some(stem) = last.strip_suffix(suffix) {
                            *last = stem.to_string();
                            break;
                        }
                    }
                    if last == "index" {
                        parts.pop();
                    }
                }
                std::iter::once(npm_name(self.package))
                    .chain(parts)
                    .collect::<Vec<_>>()
                    .join("/")
            }
        }
    }
}

/// `@types/lodash` is `lodash`, `@types/scope__name` is `@scope/name`.
fn npm_name(package: &str) -> String {
    match package.strip_prefix("@types/") {
        Some(rest) => match rest.split_once("__") {
            Some((scope, name)) => format!("@{scope}/{name}"),
            None => rest.to_string(),
        },
        None => package.to_string(),
    }
}

/// The part of `document` that `line` is in.
fn part_at(document: &Document, line: usize) -> Option<&Part> {
    document
        .parts
        .iter()
        .find(|span| span.lines.contains(&line))
        .map(|span| &span.part)
}

/// The node of `document` that `line` is in: a part of the file, or the file.
fn unit_at(document: &Document, line: usize) -> Unit {
    match part_at(document, line) {
        Some(part) => Unit::in_part(Path::new(&document.path), part.clone()),
        None => Unit::from(document.path.as_str()),
    }
}

/// A dependency of a file before it is kept, without its line.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DepKey<'a> {
    /// The file it is on; `None` for an external module.
    internal: Option<&'a str>,
    /// The external module; empty for a file of the project.
    external: String,
    /// The part of the file the line is in.
    part: Option<&'a Part>,
    written: Written,
}

/// Dependencies and symbol uses of the documents.
pub struct Facts {
    pub deps: Vec<Dep>,
    pub symbols: Vec<SymbolRef>,
    /// With `symbols`, the files that use a name of an unnamed package, whose
    /// uses are left out of `symbols`.
    pub unnamed: BTreeSet<String>,
    /// With `watched`, the qualifier and name of every symbol the index defines or
    /// references whose name is one of `watched`: what a `forbid_symbol` name can mean.
    pub known: Vec<(String, String)>,
    /// Whether `known` is whole: false when the index left some definitions unnamed.
    pub known_whole: bool,
}

/// One `Dep` per file and module it refers to, at the first line that does:
/// the import. With `symbols`, also one `SymbolRef` per use of a name.
///
/// `excluded` are documents of files that `exclude` hides: never read as
/// sources, they only resolve the names that no document of `documents` defines.
pub fn facts(documents: &[Document], excluded: &[Document], symbols: bool) -> Facts {
    facts_watching(documents, excluded, symbols, &BTreeSet::new())
}

/// [`facts`] that also collects `Facts::known` for the names in `watched`.
///
/// Every reference is read with [`read`] first, so what follows is the same for every
/// language: a module, or a name at the top level of one, is a dependency on the file
/// that defines it; every name, members included, is a symbol use.
pub fn facts_watching(
    documents: &[Document],
    excluded: &[Document],
    symbols: bool,
    watched: &BTreeSet<String>,
) -> Facts {
    let mut defined: HashMap<&str, &str> = HashMap::new();
    // Symbols that more than one file defines: a Ruby module reopened in each file under it.
    let mut reopened: HashSet<&str> = HashSet::new();
    for document in documents {
        for symbol in &document.definitions {
            let entry = defined.entry(symbol).or_insert(&document.path);
            if *entry != document.path.as_str() {
                reopened.insert(symbol);
            }
            if document.path.as_str() < *entry {
                *entry = &document.path;
            }
        }
    }
    let mut hidden: HashMap<&str, &str> = HashMap::new();
    for document in excluded {
        for symbol in &document.definitions {
            if !defined.contains_key(symbol.as_str()) {
                let entry = hidden.entry(symbol).or_insert(&document.path);
                if document.path.as_str() < *entry {
                    *entry = &document.path;
                }
            }
        }
    }
    // Packages that define something are the project's own: a reference into
    // one that no definition matches (a relative import in Python) is not external.
    let own: BTreeSet<String> = defined
        .keys()
        .chain(hidden.keys())
        .filter_map(|symbol| read(symbol)?.package)
        .collect();
    // The names defined where a reference is matched by the ending of its path, with
    // the file of each. An excluded definition is the answer only when no checked one matches.
    let by_ending = |defined: &HashMap<&str, &'_ str>| -> Vec<(Vec<String>, Vec<String>, String)> {
        defined
            .iter()
            .filter_map(|(symbol, path)| {
                let name = read(symbol)?;
                name.reading
                    .by_ending
                    .then(|| (name.path, name.inner, path.to_string()))
            })
            .collect()
    };
    let endings = [by_ending(&defined), by_ending(&hidden)];
    // The one file that defines what `name` can mean, matched by the ending of its path.
    let ending = |name: &Name| -> Option<&str> {
        if name.path.is_empty() {
            return None;
        }
        endings.iter().find_map(|names| {
            let mut found = names
                .iter()
                .filter(|(path, inner, _)| *inner == name.inner && path.ends_with(&name.path))
                .map(|(_, _, file)| file.as_str());
            let first = found.next()?;
            found.all(|file| file == first).then_some(first)
        })
    };
    // The file that defines `symbol`, the first by path when several do.
    let defined_at = |symbol: &str| defined.get(symbol).or_else(|| hidden.get(symbol)).copied();
    // The one file that defines `symbol`: a module reopened in several files is no one file's.
    let home = |symbol: &str| -> Option<&str> {
        (!reopened.contains(symbol))
            .then(|| defined_at(symbol))
            .flatten()
    };
    let in_project = |symbol: &str| defined_at(symbol).is_some();
    let checked: HashSet<&str> = documents.iter().map(|d| d.path.as_str()).collect();
    let mut deps = Vec::new();
    let mut uses = Vec::new();
    let mut unnamed = BTreeSet::new();
    // What the index defines is read as if a file of no project referenced it, so that
    // a definition no one uses still tells what a name can mean.
    let definitions = (symbols && !watched.is_empty()).then(|| Document {
        path: String::new(),
        references: defined
            .keys()
            .chain(hidden.keys())
            .enumerate()
            .map(|(line, symbol)| Reference {
                line: line + 1,
                symbol: (*symbol).to_string(),
                spelled: None,
            })
            .collect(),
        ..Document::default()
    });
    for document in documents.iter().chain(definitions.as_ref()) {
        let names: Vec<(&Reference, Name)> = document
            .references
            .iter()
            .filter_map(|reference| Some((reference, read(&reference.symbol)?)))
            .collect();
        // Where a line also references each module around the path it writes, only the
        // deepest one of a package is what it depends on.
        let mut paths: HashMap<usize, Vec<&Name>> = HashMap::new();
        for (reference, name) in &names {
            if name.shape == Shape::Module && name.reading.enclosing {
                paths.entry(reference.line).or_default().push(name);
            }
        }
        let shadowed = |name: &Name, line: usize| {
            name.shape == Shape::Module
                && name.reading.enclosing
                && paths.get(&line).is_some_and(|all| {
                    all.iter().any(|other| {
                        other.package == name.package
                            && other.path.len() > name.path.len()
                            && other.path.starts_with(&name.path)
                    })
                })
        };
        // The modules the file names, by package and path, at the first line that does: a
        // name of one adds nothing, whichever of its files defines it (a Go package). A
        // re-exported name has a module of its own, which the line does not name.
        let mut named: HashMap<(&Option<String>, &[String]), usize> = HashMap::new();
        for (reference, name) in &names {
            if name.shape == Shape::Module {
                let line = named
                    .entry((&name.package, &name.path))
                    .or_insert(reference.line);
                *line = (*line).min(reference.line);
            }
        }
        // Each dependency at its first line.
        let mut first: BTreeMap<DepKey, usize> = BTreeMap::new();
        let mine: HashSet<&str> = document.definitions.iter().map(String::as_str).collect();
        for (reference, name) in &names {
            if shadowed(name, reference.line) {
                continue;
            }
            if symbols && let Some(called) = &name.called {
                let found = home(&name.symbol).or_else(|| name.owner.as_deref().and_then(home));
                // A name of this file, or a module it opens itself (`Shop` around its
                // classes), is no use of it.
                let own_name = found == Some(document.path.as_str())
                    || (name.shape != Shape::Member && mine.contains(name.symbol.as_str()));
                if !own_name {
                    if found.is_none() && name.reading.unnamed {
                        // Which package an unnamed name belongs to the index does not say.
                        unnamed.insert(document.path.clone());
                    } else {
                        let target = match found {
                            Some(file) => Some(Target::Internal(Unit::from(file))),
                            None if in_project(&name.symbol)
                                || name.owner.as_deref().is_some_and(in_project) =>
                            {
                                None
                            }
                            None => Some(Target::External(name.external.clone())),
                        };
                        let specifier = match (&called.qualifier, found) {
                            (Qualifier::Spelled(qualifier), _) => qualifier.clone(),
                            (Qualifier::File(_), Some(file)) => file
                                .rsplit_once('.')
                                .map_or(file, |(stem, _)| stem)
                                .to_string(),
                            (Qualifier::File(otherwise), None) => otherwise.clone(),
                        };
                        uses.push(SymbolRef {
                            file: PathBuf::from(&document.path),
                            from: unit_at(document, reference.line),
                            line: reference.line,
                            method: called.method(&specifier),
                            specifier,
                            name: called.name.clone(),
                            target,
                        });
                    }
                }
            }
            if !matches!(name.shape, Shape::Module | Shape::TopLevel) {
                continue;
            }
            if name.shape == Shape::TopLevel
                && named
                    .get(&(&name.package, name.path.as_slice()))
                    .is_some_and(|line| *line <= reference.line)
            {
                continue;
            }
            let mut internal = defined_at(&name.symbol);
            if internal.is_none() && name.reading.by_ending {
                internal = ending(name);
            }
            if internal == Some(document.path.as_str()) {
                continue;
            }
            // An external name is reached through its module, which the line names too; so
            // is a name an excluded file defines (a generated `api.pb.go` beside `api.go`):
            // only a module the code names fails as excluded.
            if name.shape == Shape::TopLevel && internal.is_none_or(|file| !checked.contains(file))
            {
                continue;
            }
            let unnamed = internal.is_none() && name.reading.unnamed;
            if internal.is_none()
                && !unnamed
                && name
                    .package
                    .as_ref()
                    .is_some_and(|package| own.contains(package))
            {
                continue;
            }
            // Only the string of the import names an unnamed package; a name
            // bound to one of its modules (`z` of `import { z } from "zod"`)
            // stands on a line whose string already did.
            let module = match (internal, unnamed) {
                (Some(_), _) => String::new(),
                (None, true) => reference.spelled.clone().unwrap_or_default(),
                (None, false) => name.external.clone(),
            };
            if internal.is_none() && module.is_empty() {
                continue;
            }
            let part = part_at(document, reference.line);
            let written = match name.shape {
                Shape::Module => Written::Module,
                _ => Written::Name,
            };
            let key = DepKey {
                internal,
                external: module,
                part,
                written,
            };
            let line = first.entry(key).or_insert(reference.line);
            *line = (*line).min(reference.line);
        }
        // A name of a file that the file also names as a module no later adds nothing,
        // whatever spelling each has (Python's `src.shop.db` and `shop.db`).
        let covered = |key: &DepKey, line: usize| {
            let named = DepKey {
                written: Written::Module,
                ..key.clone()
            };
            key.written == Written::Name && first.get(&named).is_some_and(|at| *at <= line)
        };
        let kept: Vec<(DepKey, usize)> = first
            .iter()
            .filter(|(key, line)| !covered(key, **line))
            .map(|(key, line)| (key.clone(), *line))
            .collect();
        for (key, line) in kept {
            let (specifier, target) = match key.internal {
                Some(path) => (path.to_string(), Target::Internal(Unit::from(path))),
                None => (key.external.clone(), Target::External(key.external)),
            };
            let dep = Dep::module(&document.path, line, specifier, target);
            deps.push(Dep {
                from: match key.part {
                    Some(part) => Unit::in_part(Path::new(&document.path), part.clone()),
                    None => dep.from.clone(),
                },
                written: key.written,
                ..dep
            });
        }
    }
    let known_whole = !unnamed.remove("");
    deps.retain(|dep| !dep.file.as_os_str().is_empty());
    let mut known = Vec::new();
    uses.retain(|used| {
        if !used.file.as_os_str().is_empty() {
            return true;
        }
        known.push(used.clone());
        false
    });
    let known = known
        .iter()
        .chain(&uses)
        .flat_map(|used| {
            let method = used.method.clone();
            std::iter::once((used.specifier.clone(), used.name.clone())).chain(method)
        })
        .filter(|(_, name)| watched.contains(name))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Facts {
        deps,
        symbols: uses,
        unnamed,
        known,
        known_whole,
    }
}

impl Documents {
    pub fn merged(parts: Vec<Vec<Document>>) -> Result<Self> {
        let mut by_path: BTreeMap<String, Document> = BTreeMap::new();
        for document in parts.into_iter().flatten() {
            match by_path.get_mut(&document.path) {
                Some(seen) => {
                    seen.references.extend(document.references);
                    seen.definitions.extend(document.definitions);
                }
                None => {
                    by_path.insert(document.path.clone(), document);
                }
            }
        }
        if by_path.is_empty() {
            bail!("the indexers produced no documents");
        }
        Ok(Self(by_path.into_values().collect()))
    }
}

/// The references of `documents`, each from the definition that holds it to the
/// definition it names. A symbol that more than one file defines (a Ruby module
/// reopened in each file under it) names no one place, so its references are left to
/// the dependencies, which keep reaching a whole file.
pub fn flow(documents: &[Document]) -> Flow {
    let mut defined: HashMap<&str, &str> = HashMap::new();
    let mut reopened: HashSet<&str> = HashSet::new();
    let mut scoped: HashSet<&str> = HashSet::new();
    let mut flow = Flow::default();
    for document in documents {
        for symbol in &document.definitions {
            if *defined.entry(symbol).or_insert(&document.path) != document.path.as_str() {
                reopened.insert(symbol);
            }
        }
        for scope in &document.scopes {
            scoped.insert(&scope.symbol);
        }
        if !document.scopes.is_empty() {
            flow.scopes
                .insert(document.path.clone(), document.scopes.clone());
        }
    }
    // A method reaches the code of its own file that is in none: the class body and the
    // constants that run when the file loads.
    let own: Vec<Edge> = flow
        .scopes
        .iter()
        .flat_map(|(file, scopes)| {
            scopes.iter().map(|scope| Edge {
                from: Node {
                    file: file.clone(),
                    scope: Some(scope.symbol.clone()),
                },
                to: Node::file(file),
                line: scope.first,
            })
        })
        .collect();
    flow.edges.extend(own);
    for document in documents {
        // The files whose method a line names: such a line reaches that method, and a
        // reference to the class around it (`Document` of `Document.host`) adds nothing.
        let named: HashSet<(usize, &str)> = document
            .references
            .iter()
            .filter(|r| scoped.contains(r.symbol.as_str()) && !reopened.contains(r.symbol.as_str()))
            .filter_map(|r| defined.get(r.symbol.as_str()).map(|file| (r.line, *file)))
            .collect();
        for reference in &document.references {
            let symbol = reference.symbol.as_str();
            let Some(&file) = defined.get(symbol) else {
                continue;
            };
            if reopened.contains(symbol) {
                continue;
            }
            let targets = if scoped.contains(symbol) {
                vec![Node {
                    file: file.to_string(),
                    scope: Some(symbol.to_string()),
                }]
            } else if symbol.ends_with('/') || named.contains(&(reference.line, file)) {
                // An import runs the code of the file that is in no method; the names it
                // brings are reached by their own references.
                vec![Node::file(file)]
            } else {
                // A class or constant used with no method named (an opaque receiver):
                // any of the file's methods may be what runs.
                flow.nodes_of(file)
            };
            for from in flow.owners(&document.path, reference.line) {
                for to in &targets {
                    if from != *to {
                        flow.edges.push(Edge {
                            from: from.clone(),
                            to: to.clone(),
                            line: reference.line,
                        });
                    }
                }
            }
        }
    }
    flow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, defs: &[&str], refs: &[(usize, &str)]) -> Document {
        Document {
            path: path.to_string(),
            definitions: defs.iter().map(|s| s.to_string()).collect(),
            types: Vec::new(),
            scopes: Vec::new(),
            parts: Vec::new(),
            references: refs
                .iter()
                .map(|(line, symbol)| Reference {
                    line: *line,
                    symbol: symbol.to_string(),
                    spelled: None,
                })
                .collect(),
        }
    }

    const DB: &str = "scip-go gomod example.com/app . `example.com/app/internal/db`/";
    const FMT: &str = "scip-go gomod github.com/golang/go/src go1.25 fmt/";

    /// The case of #104 in one scheme: `blob` defines `Block` with a field and a
    /// method, `chunk` a function that returns one, and `gw` calls that function and
    /// uses the member on what it returns, writing no name of `blob`. The symbols are
    /// those the indexer emits (`docs/research/symbol-use-dependencies.md`).
    struct Case {
        blob: Vec<String>,
        chunk: Vec<String>,
        /// The references of `gw`: line 1 names `chunk`, line 2 calls its function,
        /// line 3 uses the members of the value it returned.
        gw: Vec<(usize, String)>,
    }

    fn case(prefix: &str, blob: &[&str], chunk: &[&str], gw: &[(usize, &str)]) -> Case {
        let full = |s: &&str| format!("{prefix}{s}");
        Case {
            blob: blob.iter().map(full).collect(),
            chunk: chunk.iter().map(full).collect(),
            gw: gw.iter().map(|(line, s)| (*line, full(s))).collect(),
        }
    }

    fn cases() -> Vec<(&'static str, Case)> {
        vec![
            (
                "java",
                case(
                    "scip-java maven maven/fx/app 1.0 ",
                    &[
                        "fx/blob/Block#",
                        "fx/blob/Block#size().",
                        "fx/blob/Block#len.",
                    ],
                    &["fx/chunk/Chunk#", "fx/chunk/Chunk#make()."],
                    &[
                        (1, "fx/chunk/Chunk#"),
                        (2, "fx/chunk/Chunk#"),
                        (2, "fx/chunk/Chunk#make()."),
                        (3, "fx/blob/Block#size()."),
                        (3, "fx/blob/Block#len."),
                    ],
                ),
            ),
            (
                "csharp",
                case(
                    "scip-dotnet nuget . . ",
                    &["Blob/Block#", "Blob/Block#Size().", "Blob/Block#Len."],
                    &["Chunk/Chunk#", "Chunk/Chunk#Make()."],
                    &[
                        (1, "Chunk/Chunk#"),
                        (2, "Chunk/"),
                        (2, "Chunk/Chunk#"),
                        (2, "Chunk/Chunk#Make()."),
                        (3, "Blob/Block#Size()."),
                        (3, "Blob/Block#Len."),
                    ],
                ),
            ),
            (
                "ruby",
                case(
                    "scip-ruby gem fx 0 ",
                    &[
                        "Blob#",
                        "Blob#Block#",
                        "Blob#Block#size().",
                        "Blob#Block#len().",
                    ],
                    &["Chunk#", "`<Class:Chunk>`#make()."],
                    &[
                        (1, "Chunk#"),
                        (2, "Chunk#"),
                        (2, "`<Class:Chunk>`#make()."),
                        (3, "Blob#Block#size()."),
                        (3, "Blob#Block#len()."),
                    ],
                ),
            ),
            (
                "rust",
                case(
                    "rust-analyzer cargo ",
                    &[
                        "blob 0.1.0 crate/",
                        "blob 0.1.0 Block#",
                        "blob 0.1.0 Block#len.",
                        "blob 0.1.0 impl#[Block]size().",
                    ],
                    &["chunk 0.1.0 crate/", "chunk 0.1.0 make()."],
                    &[
                        (1, "chunk 0.1.0 crate/"),
                        (2, "chunk 0.1.0 crate/"),
                        (2, "chunk 0.1.0 make()."),
                        (3, "blob 0.1.0 impl#[Block]size()."),
                        (3, "blob 0.1.0 Block#len."),
                    ],
                ),
            ),
            (
                "typescript",
                case(
                    "scip-typescript npm fx 1.0.0 ",
                    &[
                        "src/blob/`index.ts`/",
                        "src/blob/`index.ts`/Block#",
                        "src/blob/`index.ts`/Block#size().",
                        "src/blob/`index.ts`/Block#len.",
                    ],
                    &["src/chunk/`index.ts`/", "src/chunk/`index.ts`/make()."],
                    &[
                        (1, "src/chunk/`index.ts`/"),
                        (1, "src/chunk/`index.ts`/make()."),
                        (2, "src/chunk/`index.ts`/make()."),
                        (3, "src/blob/`index.ts`/Block#size()."),
                        (3, "src/blob/`index.ts`/Block#len."),
                    ],
                ),
            ),
            (
                "python",
                case(
                    "scip-python python fx 0.1 ",
                    &[
                        "`fx.blob`/__init__:",
                        "`fx.blob`/Block#",
                        "`fx.blob`/Block#size().",
                        "`fx.blob`/Block#len.",
                    ],
                    &["`fx.chunk`/__init__:", "`fx.chunk`/make()."],
                    &[
                        (1, "`fx.chunk`/__init__:"),
                        (1, "`fx.chunk`/make()."),
                        (2, "`fx.chunk`/make()."),
                        (3, "`fx.blob`/Block#size()."),
                        (3, "`fx.blob`/Block#len."),
                    ],
                ),
            ),
            (
                "go",
                case(
                    "scip-go gomod ex.com/fx . ",
                    &[
                        "`ex.com/fx/blob`/",
                        "`ex.com/fx/blob`/Block#",
                        "`ex.com/fx/blob`/Block#Size().",
                        "`ex.com/fx/blob`/Block#Len.",
                    ],
                    &["`ex.com/fx/chunk`/", "`ex.com/fx/chunk`/Make()."],
                    &[
                        (1, "`ex.com/fx/chunk`/"),
                        (2, "`ex.com/fx/chunk`/"),
                        (2, "`ex.com/fx/chunk`/Make()."),
                        (3, "`ex.com/fx/blob`/Block#Size()."),
                        (3, "`ex.com/fx/blob`/Block#Len."),
                    ],
                ),
            ),
        ]
    }

    /// The dependencies of `gw` and the files its symbol uses point at, by line.
    type Read = (Vec<(usize, Target)>, Vec<(usize, Option<Target>)>);

    fn read_case(case: &Case) -> Read {
        let refs: Vec<(usize, &str)> = case.gw.iter().map(|(l, s)| (*l, s.as_str())).collect();
        let blob: Vec<&str> = case.blob.iter().map(String::as_str).collect();
        let chunk: Vec<&str> = case.chunk.iter().map(String::as_str).collect();
        let docs = [
            doc("blob", &blob, &[]),
            doc("chunk", &chunk, &[]),
            doc("gw", &[], &refs),
        ];
        let read = facts(&docs, &[], true);
        let mut deps: Vec<_> = read
            .deps
            .iter()
            .map(|d| (d.line, d.target.clone()))
            .collect();
        deps.sort_by_key(|(line, target)| (*line, format!("{target:?}")));
        let mut uses: Vec<_> = read
            .symbols
            .iter()
            .map(|u| (u.line, u.target.clone()))
            .collect();
        uses.sort_by_key(|(line, target)| (*line, format!("{target:?}")));
        uses.dedup();
        (deps, uses)
    }

    #[test]
    fn a_member_on_a_value_another_module_returned_is_no_dependency_in_any_language() {
        let chunk = Target::Internal(Unit::from("chunk"));
        let blob = Target::Internal(Unit::from("blob"));
        for (language, case) in cases() {
            let (deps, uses) = read_case(&case);
            assert_eq!(deps, [(1, chunk.clone())], "{language}");
            // The member is still a use of a name, which the symbol rules read.
            assert!(
                uses.contains(&(3, Some(blob.clone()))),
                "{language}: {uses:?}"
            );
        }
    }

    /// The internal dependencies of the last document, by line and file.
    fn deps_of(docs: &[Document]) -> Vec<(usize, String)> {
        let last = &docs[docs.len() - 1].path;
        let mut found: Vec<_> = facts(docs, &[], false)
            .deps
            .into_iter()
            .filter(|d| d.file == Path::new(last))
            .map(|d| (d.line, d.specifier))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_name_reached_through_a_re_export_is_a_dependency_on_the_file_that_defines_it() {
        // Rust: `chunk` has `pub use blob::Block;`, and `gw` writes `chunk::Block` on line 6.
        let rust = "rust-analyzer cargo ";
        let rust_docs = [
            doc(
                "blob/src/lib.rs",
                &[
                    &format!("{rust}blob 0.1.0 crate/"),
                    &format!("{rust}blob 0.1.0 Block#"),
                ],
                &[],
            ),
            doc(
                "chunk/src/lib.rs",
                &[&format!("{rust}chunk 0.1.0 crate/")],
                &[],
            ),
            doc(
                "gw/src/lib.rs",
                &[],
                &[
                    (2, &format!("{rust}chunk 0.1.0 crate/")),
                    (6, &format!("{rust}chunk 0.1.0 crate/")),
                    (6, &format!("{rust}blob 0.1.0 Block#")),
                ],
            ),
        ];
        assert_eq!(
            deps_of(&rust_docs),
            [
                (2, "chunk/src/lib.rs".to_string()),
                (6, "blob/src/lib.rs".to_string())
            ]
        );
        // TypeScript: `export { Block } from "../blob"` in `chunk`, and
        // `import { Block } from "../chunk"` in `gw` on line 1, used on line 4.
        let ts = "scip-typescript npm fx 1.0.0 src/";
        let ts_docs = [
            doc(
                "src/blob/index.ts",
                &[
                    &format!("{ts}blob/`index.ts`/"),
                    &format!("{ts}blob/`index.ts`/Block#"),
                ],
                &[],
            ),
            doc(
                "src/chunk/index.ts",
                &[&format!("{ts}chunk/`index.ts`/")],
                &[],
            ),
            doc(
                "src/gw/index.ts",
                &[],
                &[
                    (1, &format!("{ts}chunk/`index.ts`/")),
                    (1, &format!("{ts}blob/`index.ts`/Block#")),
                    (4, &format!("{ts}blob/`index.ts`/Block#")),
                ],
            ),
        ];
        assert_eq!(
            deps_of(&ts_docs),
            [
                (1, "src/blob/index.ts".to_string()),
                (1, "src/chunk/index.ts".to_string())
            ]
        );
    }

    #[test]
    fn a_name_is_its_own_dependency_unless_the_file_names_its_module_no_later() {
        // `billing/index.ts` re-exports `charge` of `billing/internal/charge.ts`. Line 1
        // imports it through the index, line 2 straight from its file; `Order` of
        // `orders/model.ts` is imported by name and module on line 3.
        let ts = "scip-typescript npm fx 1.0.0 src/";
        let charge = format!("{ts}billing/internal/`charge.ts`/charge.");
        let order = format!("{ts}orders/`model.ts`/Order#");
        let docs = [
            doc(
                "src/billing/index.ts",
                &[&format!("{ts}billing/`index.ts`/")],
                &[],
            ),
            doc(
                "src/billing/internal/charge.ts",
                &[&format!("{ts}billing/internal/`charge.ts`/"), &charge],
                &[],
            ),
            doc(
                "src/orders/model.ts",
                &[&format!("{ts}orders/`model.ts`/"), &order],
                &[],
            ),
            doc(
                "src/ui/page.ts",
                &[],
                &[
                    (1, &format!("{ts}billing/`index.ts`/")),
                    (1, &charge),
                    (2, &format!("{ts}billing/internal/`charge.ts`/")),
                    (2, &charge),
                    (3, &format!("{ts}orders/`model.ts`/")),
                    (3, &order),
                    (5, &charge),
                ],
            ),
        ];
        let mut found: Vec<_> = facts(&docs, &[], false)
            .deps
            .into_iter()
            .map(|d| (d.line, d.specifier, d.written))
            .collect();
        found.sort();
        assert_eq!(
            found,
            [
                (1, "src/billing/index.ts".to_string(), Written::Module),
                (
                    1,
                    "src/billing/internal/charge.ts".to_string(),
                    Written::Name
                ),
                (
                    2,
                    "src/billing/internal/charge.ts".to_string(),
                    Written::Module
                ),
                (3, "src/orders/model.ts".to_string(), Written::Module),
            ]
        );
    }

    #[test]
    fn a_name_of_a_module_the_file_names_adds_nothing_whatever_file_of_it_defines_it() {
        // A Go package of three files: `gw` imports it on line 3 and calls two of its
        // functions on line 6, each defined in another file of it.
        let go = "scip-go gomod ex.com/fx . `ex.com/fx/blob`/";
        let docs = [
            doc("blob/a.go", &[go], &[]),
            doc("blob/make.go", &[go, &format!("{go}Make().")], &[]),
            doc("blob/other.go", &[go, &format!("{go}Other().")], &[]),
            doc(
                "gw/gw.go",
                &[],
                &[
                    (3, go),
                    (6, go),
                    (6, &format!("{go}Make().")),
                    (6, &format!("{go}Other().")),
                ],
            ),
        ];
        assert_eq!(deps_of(&docs), [(3, "blob/a.go".to_string())]);
    }

    #[test]
    fn a_name_an_excluded_file_defines_is_no_dependency_of_its_own() {
        // `api` is `api.go` and a generated `api.pb.go` that `exclude` hides; `gw` imports
        // `api` on line 3 and uses `api.Request`, defined in the generated file, on line 6.
        let api = "scip-go gomod ex.com/fx . `ex.com/fx/api`/";
        let request = format!("{api}Request#");
        let sources = [
            doc("api/api.go", &[api], &[(4, &request)]),
            doc("gw/gw.go", &[], &[(3, api), (6, &request)]),
        ];
        let excluded = [doc("api/api.pb.go", &[api, &request], &[])];
        let mut found: Vec<_> = facts(&sources, &excluded, false)
            .deps
            .into_iter()
            .map(|d| (d.file.to_str().unwrap().to_string(), d.line, d.specifier))
            .collect();
        found.sort();
        assert_eq!(
            found,
            [("gw/gw.go".to_string(), 3, "api/api.go".to_string())]
        );
    }

    #[test]
    fn a_python_name_re_exported_by_an_init_is_a_dependency_on_its_module() {
        // As scip-python writes them: a module by its path from the working directory
        // (`src.app.blob`) in its definition and as imported (`app.chunk`) in a
        // reference, a name by the path of its definition in both. `app.chunk`
        // re-exports `Block`.
        let py = "scip-python python fx 0.1 ";
        let docs = [
            doc(
                "src/app/blob/__init__.py",
                &[
                    &format!("{py}`src.app.blob`/__init__:"),
                    &format!("{py}`src.app.blob`/Block#"),
                ],
                &[],
            ),
            doc(
                "src/app/chunk/__init__.py",
                &[&format!("{py}`src.app.chunk`/__init__:")],
                &[],
            ),
            doc(
                "src/app/gw/__init__.py",
                &[],
                &[
                    (1, &format!("{py}`app.chunk`/__init__:")),
                    (1, &format!("{py}`src.app.blob`/Block#")),
                    (3, &format!("{py}`src.app.blob`/Block#size().")),
                ],
            ),
        ];
        assert_eq!(
            deps_of(&docs),
            [
                (1, "src/app/blob/__init__.py".to_string()),
                (1, "src/app/chunk/__init__.py".to_string())
            ]
        );
    }

    #[test]
    fn a_constructor_that_stands_for_the_class_written_is_a_dependency_on_the_class() {
        // scip-java gives `new X()` only as `X#<init>().`, through a wildcard import or a
        // qualified name, and a nested class is reached through the class around it.
        let java = "scip-java maven maven/fx/app 1.0 fx/blob/";
        let java_docs = [
            doc(
                "Block.java",
                &[
                    &format!("{java}Block#"),
                    &format!("{java}Block#`<init>`()."),
                ],
                &[],
            ),
            doc(
                "Outer.java",
                &[&format!("{java}Outer#"), &format!("{java}Outer#Inner#")],
                &[],
            ),
            doc(
                "Gw.java",
                &[],
                &[
                    (1, "scip-java maven . . fx/blob/"),
                    (6, &format!("{java}Block#`<init>`().")),
                    (7, &format!("{java}Outer#Inner#`<init>`().")),
                    (8, &format!("{java}Block#size().")),
                ],
            ),
        ];
        assert_eq!(
            deps_of(&java_docs),
            [(6, "Block.java".to_string()), (7, "Outer.java".to_string())]
        );
        // scip-dotnet gives an attribute `[X]` only as `XAttribute#.ctor().`, the
        // project's own and the framework's in generated `AssemblyInfo.cs` alike.
        let dotnet = "scip-dotnet nuget . . Blob/";
        let dotnet_docs = [
            doc(
                "Blob/Mark.cs",
                &[
                    &format!("{dotnet}MarkAttribute#"),
                    &format!("{dotnet}MarkAttribute#`.ctor`()."),
                ],
                &[],
            ),
            doc(
                "Gw/Marked.cs",
                &[],
                &[
                    (3, &format!("{dotnet}MarkAttribute#`.ctor`().")),
                    (
                        4,
                        "scip-dotnet nuget System.Runtime 9.0.0.0 Reflection/AssemblyCompanyAttribute#`.ctor`().",
                    ),
                    (
                        5,
                        "scip-dotnet nuget System.Runtime 9.0.0.0 Reflection/AssemblyCompanyAttribute#Name.",
                    ),
                ],
            ),
        ];
        let deps = facts(&dotnet_docs, &[], false).deps;
        let found: Vec<_> = deps.iter().map(|d| (d.line, d.target.clone())).collect();
        assert_eq!(
            found,
            [
                (
                    4,
                    Target::External("Reflection.AssemblyCompanyAttribute".into())
                ),
                (3, Target::Internal(Unit::from("Blob/Mark.cs"))),
            ]
        );
    }

    #[test]
    fn only_a_deeper_module_of_the_same_crate_hides_the_one_around_it() {
        // `chunk` has `pub use blob::inner;`, and `gw` writes `chunk::inner::deep()`.
        let rust = "rust-analyzer cargo ";
        let docs = [
            doc(
                "blob/src/inner.rs",
                &[
                    &format!("{rust}blob 0.1.0 inner/"),
                    &format!("{rust}blob 0.1.0 inner/deep()."),
                ],
                &[],
            ),
            doc(
                "blob/src/lib.rs",
                &[&format!("{rust}blob 0.1.0 crate/")],
                &[],
            ),
            doc(
                "chunk/src/lib.rs",
                &[&format!("{rust}chunk 0.1.0 crate/")],
                &[],
            ),
            doc(
                "gw/src/lib.rs",
                &[],
                &[
                    (3, &format!("{rust}chunk 0.1.0 crate/")),
                    (3, &format!("{rust}blob 0.1.0 crate/")),
                    (3, &format!("{rust}blob 0.1.0 inner/")),
                    (3, &format!("{rust}blob 0.1.0 inner/deep().")),
                ],
            ),
        ];
        assert_eq!(
            deps_of(&docs),
            [
                (3, "blob/src/inner.rs".to_string()),
                (3, "chunk/src/lib.rs".to_string())
            ]
        );
    }

    /// A member of a Rust type (a variant, a method, an associated constant) is
    /// used through the type, as a Ruby method is through its class, so
    /// `forbid_symbol` can name it as `path::Type.member`.
    #[test]
    fn a_member_of_a_rust_type_is_named_through_the_type() {
        let rust = "rust-analyzer cargo ";
        let docs = [doc(
            "src/main.rs",
            &[&format!("{rust}app 0.1.0 crate/")],
            &[
                (3, &format!("{rust}std 1.0.0 sync/atomic/Ordering#")),
                (3, &format!("{rust}std 1.0.0 sync/atomic/Ordering#Relaxed.")),
                (4, &format!("{rust}db 0.1.0 client/Client#")),
                (4, &format!("{rust}db 0.1.0 client/Client#query().")),
            ],
        )];
        let uses: Vec<_> = facts(&docs, &[], true)
            .symbols
            .into_iter()
            .map(|u| {
                (
                    u.line,
                    format!("{}.{}", u.specifier, u.name),
                    u.method.map(|(owner, name)| format!("{owner}.{name}")),
                )
            })
            .collect();
        let method = |m: &str| Some(m.to_string());
        assert_eq!(
            uses,
            [
                (3, "std::sync::atomic.Ordering".into(), None),
                (
                    3,
                    "std::sync::atomic.Ordering".into(),
                    method("std::sync::atomic::Ordering.Relaxed")
                ),
                (4, "db::client.Client".into(), None),
                (
                    4,
                    "db::client.Client".into(),
                    method("db::client::Client.query")
                ),
            ]
        );
    }

    #[test]
    fn the_same_shapes_make_the_same_facts_whatever_the_scheme() {
        let all: Vec<_> = cases()
            .into_iter()
            .map(|(l, c)| (l, read_case(&c)))
            .collect();
        let (first, expected) = &all[0];
        for (language, read) in &all[1..] {
            // A file's module is a class in Java and C# and a constant in Ruby, so they name
            // `chunk` as a name too; the dependencies and the members are the same.
            assert_eq!(read.0, expected.0, "{language} and {first}");
            let members = |uses: &[(usize, Option<Target>)]| {
                uses.iter()
                    .filter(|(line, _)| *line == 3)
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                members(&read.1),
                members(&expected.1),
                "{language} and {first}"
            );
        }
    }

    #[test]
    fn a_use_in_a_cfg_test_part_is_a_dependency_of_that_part_apart_from_the_file() {
        let testkit = "rust-analyzer cargo testkit 0.1.0 crate/";
        let mut document = doc("src/blob_db.rs", &[], &[(1, testkit), (9, testkit)]);
        document.parts = crate::lang::parts::rust(
            "src/blob_db.rs",
            "pub fn open() {}\n\n#[cfg(test)]\nmod tests {\n    fn a() {}\n    fn b() {}\n    fn c() {}\n    fn d() {}\n    fn e() {}\n}\n",
        )
        .spans;
        let deps = facts(&[document], &[], false).deps;
        let found: Vec<_> = deps
            .iter()
            .map(|d| (d.line, d.from.is_test(), d.file.to_str().unwrap()))
            .collect();
        assert_eq!(
            found,
            [(1, false, "src/blob_db.rs"), (9, true, "src/blob_db.rs")]
        );
    }

    #[test]
    fn a_symbol_used_in_a_cfg_test_part_leaves_the_part() {
        let tablet = "rust-analyzer cargo testkit 0.1.0 tablet().";
        let mut document = doc("src/blob_db.rs", &[], &[(1, tablet), (9, tablet)]);
        document.parts = crate::lang::parts::rust(
            "src/blob_db.rs",
            "pub fn open() {}\n\n#[cfg(test)]\nmod tests {\n    fn a() {}\n    fn b() {}\n    fn c() {}\n    fn d() {}\n    fn e() {}\n}\n",
        )
        .spans;
        let uses = facts(&[document], &[], true).symbols;
        let found: Vec<_> = uses.iter().map(|u| (u.line, u.from.is_test())).collect();
        assert_eq!(found, [(1, false), (9, true)]);
    }

    #[test]
    fn ruby_depends_on_the_deepest_constant_of_a_line() {
        let order = "scip-ruby gem project 0 Shop#Domain#Order#";
        let docs = [
            doc("lib/order.rb", &[order], &[]),
            doc(
                "lib/web.rb",
                &[],
                &[
                    (3, "scip-ruby gem project 0 Shop#"),
                    (3, "scip-ruby gem project 0 Shop#Domain#"),
                    (3, order),
                    (3, "scip-ruby gem project 0 Class#new()."),
                    (4, "scip-ruby gem project 0 Shop#Domain#Order#`@id`."),
                ],
            ),
        ];
        let deps = facts(&docs, &[], false).deps;
        let found: Vec<_> = deps
            .iter()
            .map(|d| (d.line, d.specifier.as_str()))
            .collect();
        assert_eq!(found, [(3, "lib/order.rb")]);
    }

    #[test]
    fn a_ruby_class_method_depends_through_the_constant_it_is_called_on() {
        // `Shop::Domain::Order.find` on line 5, as scip-ruby emits it: the constants of the
        // path beside the method of the singleton class. A method alone, called on a
        // value, depends on nothing.
        let ruby = "scip-ruby gem project 0 ";
        let order = format!("{ruby}Shop#Domain#Order#");
        let find = format!("{ruby}Shop#Domain#`<Class:Order>`#find().");
        let docs = [
            doc("lib/order.rb", &[&order, &find], &[]),
            doc(
                "lib/web.rb",
                &[],
                &[
                    (5, &format!("{ruby}Shop#")),
                    (5, &format!("{ruby}Shop#Domain#")),
                    (5, &order),
                    (5, &find),
                    (6, &find),
                ],
            ),
        ];
        let deps = facts(&docs, &[], false).deps;
        let found: Vec<_> = deps
            .iter()
            .map(|d| (d.line, d.specifier.as_str()))
            .collect();
        assert_eq!(found, [(5, "lib/order.rb")]);
    }

    #[test]
    fn ruby_constants_and_methods_are_symbol_uses() {
        let ruby = "scip-ruby gem project 0 ";
        let client = format!("{ruby}Shop#Db#Client#");
        let query = format!("{ruby}Shop#Db#Client#query().");
        let connect = format!("{ruby}Shop#Db#`<Class:Client>`#connect().");
        let helper = format!("{ruby}Shop#`<Class:Db>`#helper().");
        let config = format!("{ruby}`<Class:Shop>`#config().");
        let docs = [
            doc(
                "lib/shop/db/client.rb",
                &[
                    &format!("{ruby}Shop#"),
                    &format!("{ruby}Shop#Db#"),
                    &client,
                    &query,
                    &connect,
                    &helper,
                ],
                &[],
            ),
            doc(
                "lib/shop/db/rollback.rb",
                &[&format!("{ruby}Shop#"), &format!("{ruby}Shop#Db#")],
                &[],
            ),
            doc("lib/shop.rb", &[&format!("{ruby}Shop#"), &config], &[]),
            doc(
                "lib/shop/web/controller.rb",
                &[&format!("{ruby}Shop#"), &format!("{ruby}Shop#Web#")],
                &[
                    (7, &format!("{ruby}Shop#")),
                    (7, &format!("{ruby}Shop#Db#")),
                    (7, &client),
                    (7, &connect),
                    (8, &query),
                    (9, &format!("{ruby}Kernel#raise().")),
                    (9, &format!("{ruby}ActiveRecord#")),
                    (9, &format!("{ruby}ActiveRecord#Rollback#")),
                    (12, &format!("{ruby}Shop#")),
                    (12, &format!("{ruby}Shop#Db#")),
                    (12, &helper),
                    (14, &config),
                ],
            ),
        ];
        let uses: Vec<_> = facts(&docs, &[], true)
            .symbols
            .into_iter()
            .map(|u| {
                let method = u.method.map(|(owner, name)| format!("{owner}.{name}"));
                (
                    u.line,
                    format!("{}.{}", u.specifier, u.name),
                    method,
                    u.target,
                )
            })
            .collect();
        let db = || Some(Target::Internal(Unit::from("lib/shop/db/client.rb")));
        let method = |m: &str| Some(m.to_string());
        assert_eq!(
            uses,
            [
                (7, "Shop::Db.Client".into(), None, db()),
                (
                    7,
                    "Shop::Db.Client".into(),
                    method("Shop::Db::Client.connect"),
                    db()
                ),
                (
                    8,
                    "Shop::Db.Client".into(),
                    method("Shop::Db::Client.query"),
                    db()
                ),
                (
                    9,
                    ".Kernel".into(),
                    method("Kernel.raise"),
                    Some(Target::External("Kernel".into()))
                ),
                (
                    9,
                    "ActiveRecord.Rollback".into(),
                    None,
                    Some(Target::External("ActiveRecord::Rollback".into()))
                ),
                // Shop::Db is reopened in both files of lib/shop/db: no one file owns it.
                (12, "Shop.Db".into(), None, None),
                (12, "Shop.Db".into(), method("Shop::Db.helper"), db()),
                // The file opens Shop too, but the method is defined elsewhere.
                (
                    14,
                    ".Shop".into(),
                    method("Shop.config"),
                    Some(Target::Internal(Unit::from("lib/shop.rb")))
                ),
            ]
        );
        assert!(facts(&docs, &[], false).symbols.is_empty());
    }

    #[test]
    fn a_type_is_the_last_descriptor_ending_in_a_hash() {
        let last = |symbol: &str| {
            split(symbol).and_then(|p| {
                p.descriptors
                    .last()
                    .map(|(n, k)| (n.clone(), matches!(k, Kind::Type)))
            })
        };
        assert_eq!(
            last("scip-go gomod ex.com/m . `ex.com/m/repo`/OrderRepository#"),
            Some(("OrderRepository".to_string(), true))
        );
        assert_eq!(
            last("semanticdb maven . . m/Outer#Inner#"),
            Some(("Inner".to_string(), true))
        );
        assert_eq!(
            last("scip-go gomod ex.com/m . `ex.com/m/repo`/OrderRepository#Find()."),
            Some(("Find".to_string(), false))
        );
    }

    #[test]
    fn parses_descriptors() {
        let d = descriptors("src/lib/`index.ts`/Client#Query().").unwrap();
        let names: Vec<_> = d.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["src", "lib", "index.ts", "Client", "Query"]);
        assert_eq!(d[2].1, Kind::Namespace);
        assert_eq!(d[3].1, Kind::Type);
        assert_eq!(d[4].1, Kind::Other);
        let go = split("scip-go gomod example.com/app . `example.com/app/internal/db`/Scoped().")
            .unwrap();
        assert_eq!(go.namespace("/"), "example.com/app/internal/db");
        let scoped =
            read("scip-go gomod example.com/app . `example.com/app/internal/db`/Scoped().")
                .unwrap();
        assert_eq!(
            (scoped.shape, scoped.inner),
            (Shape::TopLevel, vec!["Scoped".to_string()])
        );
        assert_eq!(read(DB).unwrap().shape, Shape::Module);
    }

    #[test]
    fn module_references_are_dependencies_at_their_first_line() {
        let documents = vec![
            doc("internal/db/db.go", &[DB, &format!("{DB}Client#")], &[]),
            doc(
                "internal/handlers/h.go",
                &[],
                &[
                    (12, DB),
                    (6, DB),
                    (6, FMT),
                    (9, &format!("{DB}Client#")),
                    (
                        9,
                        "scip-go gomod github.com/golang/go/src go1.25 fmt/Println().",
                    ),
                ],
            ),
        ];
        let facts = facts(&documents, &[], false);
        let got: Vec<_> = facts
            .deps
            .iter()
            .map(|d| (d.file.to_str().unwrap(), d.line, d.target.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("internal/handlers/h.go", 6, Target::External("fmt".into())),
                (
                    "internal/handlers/h.go",
                    6,
                    Target::Internal(Unit::from("internal/db/db.go"))
                ),
            ]
        );
    }

    #[test]
    fn excluded_documents_only_resolve_what_no_source_defines() {
        let generated = "scip-go gomod example.com/app . `example.com/app/gen`/";
        let sources = vec![
            doc("internal/db/db.go", &[DB], &[]),
            doc("internal/handlers/h.go", &[], &[(3, DB), (4, generated)]),
        ];
        // `a_gen` sorts first and also defines DB; it must not win.
        let excluded = vec![
            doc("a_gen/db.go", &[DB], &[(1, DB)]),
            doc("gen/gen.go", &[generated], &[]),
        ];
        let got: Vec<_> = facts(&sources, &excluded, false)
            .deps
            .iter()
            .map(|d| (d.file.to_str().unwrap().to_string(), d.target.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "internal/handlers/h.go".to_string(),
                    Target::Internal(Unit::from("gen/gen.go"))
                ),
                (
                    "internal/handlers/h.go".to_string(),
                    Target::Internal(Unit::from("internal/db/db.go"))
                ),
            ]
        );
    }

    #[test]
    fn member_references_are_symbol_uses() {
        let documents = vec![
            doc("internal/db/db.go", &[DB, &format!("{DB}Client#")], &[]),
            doc(
                "internal/handlers/h.go",
                &[],
                &[
                    (9, &format!("{DB}Client#")),
                    (10, &format!("{FMT}Println().")),
                ],
            ),
        ];
        let uses = facts(&documents, &[], true).symbols;
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].name, "Client");
        assert_eq!(uses[0].specifier, "example.com/app/internal/db");
        assert_eq!(
            uses[0].target,
            Some(Target::Internal(Unit::from("internal/db/db.go")))
        );
        assert_eq!(uses[1].target, Some(Target::External("fmt".into())));
        assert!(facts(&documents, &[], false).symbols.is_empty());
    }

    #[test]
    fn a_class_is_the_module_in_php() {
        let documents = vec![
            doc(
                "src/Domain/Order.php",
                &["scip-php composer __root__ 1.0 App/Domain/Order#"],
                &[],
            ),
            // `use function App\Domain\total;` names a function at the top level of a
            // namespace: a dependency on its file, like a class.
            doc(
                "src/Domain/functions.php",
                &["scip-php composer __root__ 1.0 App/Domain/total()."],
                &[],
            ),
            doc(
                "src/Web/C.php",
                &[],
                &[
                    (4, "scip-php composer __root__ 1.0 App/Domain/Order#"),
                    (
                        5,
                        "scip-php composer illuminate/support 10.0 Illuminate/Support/Str#",
                    ),
                    (6, "scip-php composer __root__ 1.0 App/Domain/total()."),
                    (7, "scip-php composer __root__ 1.0 App/Domain/Order#id()."),
                ],
            ),
        ];
        let deps = facts(&documents, &[], false).deps;
        let got: Vec<_> = deps.iter().map(|d| (d.line, d.target.clone())).collect();
        assert_eq!(
            got,
            vec![
                (5, Target::External("Illuminate\\Support\\Str".into())),
                (4, Target::Internal(Unit::from("src/Domain/Order.php"))),
                (6, Target::Internal(Unit::from("src/Domain/functions.php"))),
            ]
        );
    }

    #[test]
    fn typescript_externals_use_the_npm_name() {
        assert_eq!(npm_name("@types/lodash"), "lodash");
        assert_eq!(npm_name("@types/babel__core"), "@babel/core");
        assert_eq!(npm_name("react"), "react");
        let documents = vec![doc(
            "src/a.ts",
            &[],
            &[(1, "scip-typescript npm @types/lodash 4.0.0 `index.d.ts`/")],
        )];
        let facts = facts(&documents, &[], false);
        assert_eq!(facts.deps[0].target, Target::External("lodash".into()));
        let deep = split("scip-typescript npm moment 2.0.0 locale/`ru.d.ts`/").unwrap();
        assert_eq!(deep.external_module(), "moment/locale/ru");
    }

    #[test]
    fn a_name_of_an_unnamed_package_is_no_symbol_use_but_is_reported_as_unjudged() {
        // scip-typescript before 0.6.1 indexes what sits under a `package.json` without
        // `name` (`{"type": "module"}` in `esm/`) as the unnamed package `. .`.
        let documents = vec![doc(
            "src/web/a.ts",
            &[],
            &[(1, "scip-typescript npm . . esm/`index.d.ts`/Thing#")],
        )];
        let facts = facts(&documents, &[], true);
        assert!(facts.symbols.is_empty());
        assert_eq!(
            facts.unnamed.into_iter().collect::<Vec<_>>(),
            ["src/web/a.ts"]
        );
    }

    #[test]
    fn an_unnamed_package_is_what_the_string_of_its_import_spells() {
        let at = |line: i32, start: i32, end: i32| {
            let mut occurrence = Occurrence::new();
            occurrence.range = vec![line, start, end];
            occurrence
        };
        let lines: Vec<String> = [
            "import { Hono } from 'hono'",
            "import x from \"./local\"",
            // `é` is one UTF-16 unit and two UTF-8 bytes.
            "/*é*/ import a from \"@scope/pkg/sub\"",
        ]
        .map(str::to_string)
        .to_vec();
        let utf16 = PositionEncoding::UnspecifiedPositionEncoding;
        assert_eq!(
            spelled_module(&lines, &at(0, 21, 27), utf16).as_deref(),
            Some("hono")
        );
        assert_eq!(spelled_module(&lines, &at(0, 9, 13), utf16), None, "a name");
        assert_eq!(
            spelled_module(&lines, &at(1, 14, 23), utf16),
            None,
            "a path"
        );
        assert_eq!(
            spelled_module(&lines, &at(2, 20, 36), utf16).as_deref(),
            Some("@scope/pkg/sub")
        );
        let utf8 = PositionEncoding::UTF8CodeUnitOffsetFromLineStart;
        assert_eq!(
            spelled_module(&lines, &at(2, 21, 37), utf8).as_deref(),
            Some("@scope/pkg/sub")
        );
        assert_eq!(
            spelled_module(&lines, &at(9, 0, 1), utf16),
            None,
            "no such line"
        );
    }

    #[test]
    fn documents_outside_the_project_are_dropped_and_paths_rebased() {
        let mut index = Index::new();
        for path in ["a.go", "../../go-build/x.go", "C:\\cache\\y.go", "b\\c.go"] {
            let mut document = scip::types::Document::new();
            document.relative_path = path.to_string();
            let mut occurrence = Occurrence::new();
            occurrence.symbol = DB.to_string();
            occurrence.typed_range =
                Some(Typed_range::SingleLineRange(scip::types::SingleLineRange {
                    line: 4,
                    ..Default::default()
                }));
            document.occurrences.push(occurrence);
            index.documents.push(document);
        }
        let bytes = index.write_to_bytes().unwrap();
        let documents =
            parse(&bytes, Path::new("/nonexistent"), Path::new("services/api")).unwrap();
        let paths: Vec<_> = documents.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["services/api/a.go", "services/api/b/c.go"]);
        assert_eq!(documents[0].references[0].line, 5);
    }
}
