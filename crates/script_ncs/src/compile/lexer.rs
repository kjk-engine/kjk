//! NWScript's words: names, numbers, strings, the operators and the
//! `#include` and `#define` lines, with the line each is on. Comments
//! (`//` to the end of the line, `/* */`) are left out.

use super::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Name(String),
    Int(i32),
    Float(f32),
    /// The string's bytes, its escapes read.
    Str(Vec<u8>),
    /// An operator or a mark (`+=`, `(`, `;`).
    Mark(&'static str),
    /// `#include "name"`.
    Include(String),
    /// `#define NAME text`.
    Define(String, String),
    End,
}

/// A word as a mistake names it: `SendMessageToPC`, `;`, the end of the
/// file.
impl std::fmt::Display for Tok {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tok::Name(name) => write!(f, "`{name}`"),
            Tok::Int(value) => write!(f, "`{value}`"),
            Tok::Float(value) => write!(f, "`{value}`"),
            Tok::Str(bytes) => {
                let text: String = bytes.iter().map(|&byte| char::from(byte)).collect();
                write!(f, "the string {text:?}")
            }
            Tok::Mark(mark) => write!(f, "`{mark}`"),
            Tok::Include(name) => write!(f, "`#include \"{name}\"`"),
            Tok::Define(name, _) => write!(f, "`#define {name}`"),
            Tok::End => write!(f, "the end of the file"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

/// The marks, the longest first, so `<<=` is not read as `<<` and `=`.
const MARKS: [&str; 43] = [
    ">>>=", "<<=", ">>=", ">>>", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "++", "--", "+=",
    "-=", "*=", "/=", "%=", "&=", "|=", "^=", "+", "-", "*", "/", "%", "<", ">", "=", "!", "~",
    "&", "|", "^", "(", ")", "{", "}", "[", "]", ";", ",",
];
const MORE_MARKS: [&str; 3] = [".", ":", "?"];

/// The words of `source` (in the games' code page, a byte a character).
pub fn lex(source: &[u8], file: &str) -> Result<Vec<Token>, Error> {
    let mut out = Vec::new();
    let mut at = 0usize;
    let mut line = 1u32;
    let error = |line: u32, what: String| Error {
        file: file.to_string(),
        line,
        what,
    };
    let text = |range: std::ops::Range<usize>| -> String {
        source[range].iter().map(|&byte| char::from(byte)).collect()
    };
    while at < source.len() {
        let byte = source[at];
        if byte == b'\n' {
            line += 1;
            at += 1;
            continue;
        }
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if source[at..].starts_with(b"//") {
            while at < source.len() && source[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if source[at..].starts_with(b"/*") {
            at += 2;
            while at < source.len() && !source[at..].starts_with(b"*/") {
                if source[at] == b'\n' {
                    line += 1;
                }
                at += 1;
            }
            at = (at + 2).min(source.len());
            continue;
        }
        if byte == b'#' {
            let start = at;
            while at < source.len() && source[at] != b'\n' {
                at += 1;
            }
            let directive = text(start..at);
            let directive = directive.trim();
            if let Some(rest) = directive.strip_prefix("#include") {
                let name = rest.trim().trim_matches('"').trim().to_ascii_lowercase();
                let name = name.strip_suffix(".nss").unwrap_or(&name).to_string();
                out.push(Token {
                    tok: Tok::Include(name),
                    line,
                });
            } else if let Some(rest) = directive.strip_prefix("#define") {
                let mut words = rest.trim().splitn(2, char::is_whitespace);
                let name = words.next().unwrap_or("").to_string();
                let value = words.next().unwrap_or("").trim().to_string();
                out.push(Token {
                    tok: Tok::Define(name, value),
                    line,
                });
            }
            continue;
        }
        if byte == b'"' {
            at += 1;
            let mut string = Vec::new();
            loop {
                let Some(&next) = source.get(at) else {
                    return Err(error(line, "a string does not end".into()));
                };
                at += 1;
                match next {
                    b'"' => break,
                    b'\n' => {
                        return Err(error(line, "a string ends at the end of its line".into()));
                    }
                    b'\\' => {
                        let Some(&escaped) = source.get(at) else {
                            return Err(error(line, "a string does not end".into()));
                        };
                        at += 1;
                        // `\x` and two hex digits is the byte they name,
                        // which the decompiler writes for a byte that
                        // prints nothing. Without two hex digits it is a
                        // plain `x`, as any other letter after a backslash
                        // is itself.
                        let named = (escaped == b'x')
                            .then(|| source.get(at..at + 2))
                            .flatten()
                            .and_then(|digits| std::str::from_utf8(digits).ok())
                            .and_then(|digits| u8::from_str_radix(digits, 16).ok());
                        if let Some(byte) = named {
                            at += 2;
                            string.push(byte);
                            continue;
                        }
                        string.push(match escaped {
                            b'n' => b'\n',
                            b't' => b'\t',
                            b'r' => b'\r',
                            other => other,
                        });
                    }
                    other => string.push(other),
                }
            }
            out.push(Token {
                tok: Tok::Str(string),
                line,
            });
            continue;
        }
        if byte.is_ascii_digit()
            || (byte == b'.' && source.get(at + 1).is_some_and(u8::is_ascii_digit))
        {
            let start = at;
            if source[at..].starts_with(b"0x") || source[at..].starts_with(b"0X") {
                at += 2;
                while at < source.len() && source[at].is_ascii_hexdigit() {
                    at += 1;
                }
                let digits = text(start + 2..at);
                let value = u32::from_str_radix(&digits, 16)
                    .map_err(|_| error(line, format!("0x{digits} is no number")))?;
                out.push(Token {
                    tok: Tok::Int(value as i32),
                    line,
                });
                continue;
            }
            let mut float = false;
            while at < source.len() && (source[at].is_ascii_digit() || source[at] == b'.') {
                float |= source[at] == b'.';
                at += 1;
            }
            let digits = text(start..at);
            if at < source.len() && (source[at] == b'f' || source[at] == b'F') {
                float = true;
                at += 1;
            }
            let tok = if float {
                Tok::Float(
                    digits
                        .parse::<f32>()
                        .or_else(|_| format!("{digits}0").parse::<f32>())
                        .map_err(|_| error(line, format!("{digits} is no number")))?,
                )
            } else {
                Tok::Int(
                    digits
                        .parse::<i64>()
                        .map(|value| value as i32)
                        .map_err(|_| error(line, format!("{digits} is no number")))?,
                )
            };
            out.push(Token { tok, line });
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = at;
            while at < source.len() && (source[at].is_ascii_alphanumeric() || source[at] == b'_') {
                at += 1;
            }
            out.push(Token {
                tok: Tok::Name(text(start..at)),
                line,
            });
            continue;
        }
        let rest = &source[at..];
        let Some(mark) = MARKS
            .iter()
            .chain(MORE_MARKS.iter())
            .find(|mark| rest.starts_with(mark.as_bytes()))
        else {
            return Err(error(
                line,
                format!("{:?} is not NWScript", char::from(byte)),
            ));
        };
        at += mark.len();
        out.push(Token {
            tok: Tok::Mark(mark),
            line,
        });
    }
    out.push(Token {
        tok: Tok::End,
        line,
    });
    Ok(out)
}
