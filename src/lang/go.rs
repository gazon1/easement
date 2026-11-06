//! Go beyond its index: which files are programs of their own, the imports a
//! file writes, and the build tags the indexer builds with.

use super::Import;

/// A token of the head of a Go file, as far as its imports need.
#[derive(Debug, PartialEq)]
enum Token {
    Ident(String),
    /// A string literal, interpreted or raw, without its quotes.
    Str(String),
    Punct(char),
}

/// The tokens of Go source with the 1-based line each starts on; comments and
/// white space are skipped.
struct Tokens<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
}

impl<'a> Tokens<'a> {
    fn new(text: &'a str) -> Self {
        Tokens {
            chars: text.chars().peekable(),
            line: 1,
        }
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    /// Up to and past `close`, skipping what a `\` escapes when `escapes`.
    fn until(&mut self, close: char, escapes: bool) -> String {
        let mut text = String::new();
        while let Some(c) = self.bump() {
            match c {
                '\\' if escapes => {
                    text.push(c);
                    text.extend(self.bump());
                }
                c if c == close => break,
                c => text.push(c),
            }
        }
        text
    }
}

impl Iterator for Tokens<'_> {
    type Item = (usize, Token);

    fn next(&mut self) -> Option<(usize, Token)> {
        loop {
            let c = *self.chars.peek()?;
            let line = self.line;
            if c.is_whitespace() {
                self.bump();
                continue;
            }
            self.bump();
            let token = match c {
                '/' if self.chars.peek() == Some(&'/') => {
                    while self.chars.peek().is_some_and(|c| *c != '\n') {
                        self.bump();
                    }
                    continue;
                }
                '/' if self.chars.peek() == Some(&'*') => {
                    self.bump();
                    let mut star = false;
                    while let Some(c) = self.bump() {
                        if star && c == '/' {
                            break;
                        }
                        star = c == '*';
                    }
                    continue;
                }
                '"' => Token::Str(self.until('"', true)),
                '`' => Token::Str(self.until('`', false)),
                '\'' => Token::Str(self.until('\'', true)),
                c if c.is_alphanumeric() || c == '_' => {
                    let mut ident = c.to_string();
                    while let Some(&c) = self.chars.peek()
                        && (c.is_alphanumeric() || c == '_')
                    {
                        ident.push(c);
                        self.bump();
                    }
                    Token::Ident(ident)
                }
                c => Token::Punct(c),
            };
            return Some((line, token));
        }
    }
}

/// The imports the text of a Go file writes, at the line of each path, as
/// spelled and resolved by nothing: those of its `import` declarations, single
/// and grouped, with any name, `.` or `_` before the path. cgo's `import "C"`
/// is no package, so it is left out.
pub fn written_imports(text: &str) -> Vec<Import> {
    let mut tokens = Tokens::new(text);
    let mut imports = Vec::new();
    // The package clause comes first; the imports follow it, before any other declaration.
    if tokens
        .next()
        .is_none_or(|(_, t)| t != Token::Ident("package".into()))
    {
        return imports;
    }
    tokens.next();
    let mut spec = |tokens: &mut Tokens, first: (usize, Token)| {
        let (line, token) = match first {
            (_, Token::Ident(_) | Token::Punct('.')) => match tokens.next() {
                Some(next) => next,
                None => return,
            },
            other => other,
        };
        if let Token::Str(path) = token
            && path != "C"
        {
            imports.push(Import {
                specifier: path,
                line,
            });
        }
    };
    while let Some((_, token)) = tokens.next() {
        match token {
            Token::Punct(';') => {}
            Token::Ident(keyword) if keyword == "import" => match tokens.next() {
                Some((_, Token::Punct('('))) => {
                    while let Some(next) = tokens.next() {
                        match next.1 {
                            Token::Punct(')') => break,
                            Token::Punct(';') => {}
                            _ => spec(&mut tokens, next),
                        }
                    }
                }
                Some(first) => spec(&mut tokens, first),
                None => break,
            },
            _ => break,
        }
    }
    imports
}

/// Whether the text of a Go file puts it in `package main`: a program of its own.
pub fn program(text: &str) -> bool {
    let mut tokens = Tokens::new(text).map(|(_, token)| token);
    tokens.next() == Some(Token::Ident("package".into()))
        && tokens.next() == Some(Token::Ident("main".into()))
}

/// Whether `tag` is a build tag `go build -tags` takes: letters, digits, `_` and `.`.
pub fn build_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// The environment that has the indexer build with `tags`: `-tags` added to the
/// `GOFLAGS` easement runs with.
pub fn tags_env(tags: &[String]) -> (&'static str, String) {
    let mut flags = std::env::var("GOFLAGS").unwrap_or_default();
    if !flags.is_empty() {
        flags.push(' ');
    }
    flags.push_str(&format!("-tags={}", tags.join(",")));
    ("GOFLAGS", flags)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_build_tag_is_letters_digits_underscores_and_dots() {
        for tag in ["race", "e2e_1", "go1.22"] {
            assert!(super::build_tag(tag), "{tag}");
        }
        for tag in ["a b", "a,b", "", "-x"] {
            assert!(!super::build_tag(tag), "{tag}");
        }
    }

    /// `(line, module)` of each import `text` writes.
    fn written(text: &str) -> Vec<(usize, String)> {
        super::written_imports(text)
            .into_iter()
            .map(|import| (import.line, import.specifier))
            .collect()
    }

    #[test]
    fn written_imports_are_every_form_at_the_line_of_its_path() {
        let text = "/* License\n   import \"not/this\"\n*/\n//go:build linux\n\n// Package app.\npackage app\n\nimport \"fmt\"\nimport lb \"example.com/m/lib\"\n\nimport (\n\t// a comment, \"not/this\"\n\t. \"example.com/m/dot\" /* dot */\n\t_ \"strings\"\n\n\traw `example.com/m/raw`\n\t\"example.com/ext\" // trailing\n)\n\nimport (\"os\"; \"io\")\n\nfunc A() { _ = \"import \\\"x\\\"\" }\n";
        assert_eq!(
            written(text),
            [
                (9, "fmt"),
                (10, "example.com/m/lib"),
                (14, "example.com/m/dot"),
                (15, "strings"),
                (17, "example.com/m/raw"),
                (18, "example.com/ext"),
                (21, "os"),
                (21, "io"),
            ]
            .map(|(line, module)| (line, module.to_string()))
        );
    }

    #[test]
    fn cgo_is_no_written_import() {
        let text =
            "package c\n\n// #include <stdio.h>\nimport \"C\"\n\nimport (\n\t\"C\"\n\t\"os\"\n)\n";
        assert_eq!(written(text), [(8, "os".to_string())]);
    }

    #[test]
    fn imports_end_at_the_first_other_declaration() {
        let text = "package a\n\nimport \"fmt\"\n\nvar s = `\nimport \"no\"\n`\n\nfunc f() {}\n";
        assert_eq!(written(text), [(3, "fmt".to_string())]);
        assert!(written("package a\n\nfunc f() {}\n").is_empty());
    }

    #[test]
    fn a_program_is_in_package_main() {
        assert!(super::program("// tool\npackage main\n\nfunc main() {}\n"));
        assert!(!super::program("package mainly\n"));
        assert!(!super::program("package app\n"));
        // The package clause is a token, not a line: an import comment may follow
        // it, and a comment before it is no clause.
        assert!(super::program(
            "package main // import \"example.com/tool\"\n"
        ));
        assert!(super::program("package/* a */main\n"));
        assert!(!super::program("/*\npackage main\n*/\npackage app\n"));
    }
}
