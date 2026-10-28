//! The brace-structured text shared by Jedi Academy's `.shader`, `.sab` and
//! `.efx` files: words separated by white space, `//` and `/* */`
//! comments, quoted strings as single words, and `{ … }` blocks. A block is
//! named by the words just before its `{`. Words keep their case.

use std::fmt;

/// Blocks nest at most this deep; the games' files nest two or three.
/// Readers walk blocks recursively, so a deeper file would overflow the
/// stack.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    /// The words before `{`, e.g. `["textures/foo/bar"]` or `["Particle"]`.
    pub name: Vec<String>,
    /// Lines of words directly inside the block, in order.
    pub lines: Vec<Vec<String>>,
    pub children: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    /// A `}` with no open block, at a 1-based line.
    UnexpectedClose(usize),
    /// Blocks still open at the end of the file.
    Unclosed(usize),
    /// A quoted string or comment that never ends.
    Unterminated(usize),
    /// A `{` nested more than `MAX_DEPTH` deep, at a 1-based line.
    TooDeep(usize),
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptError::UnexpectedClose(line) => write!(f, "`}}` without a block at line {line}"),
            ScriptError::Unclosed(count) => write!(f, "{count} block(s) never closed"),
            ScriptError::Unterminated(line) => {
                write!(f, "unterminated string or comment from line {line}")
            }
            ScriptError::TooDeep(line) => {
                write!(f, "blocks nested deeper than {MAX_DEPTH} at line {line}")
            }
        }
    }
}

impl std::error::Error for ScriptError {}

enum Token {
    Word(String),
    Open,
    Close,
    Newline,
}

fn tokens(text: &str) -> Result<Vec<(Token, usize)>, ScriptError> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                out.push((Token::Newline, line));
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                let start = line;
                i += 2;
                loop {
                    match chars.get(i) {
                        None => return Err(ScriptError::Unterminated(start)),
                        Some('*') if chars.get(i + 1) == Some(&'/') => {
                            i += 2;
                            break;
                        }
                        Some('\n') => {
                            line += 1;
                            i += 1;
                        }
                        Some(_) => i += 1,
                    }
                }
            }
            '{' => {
                out.push((Token::Open, line));
                i += 1;
            }
            '}' => {
                out.push((Token::Close, line));
                i += 1;
            }
            '"' => {
                let start = line;
                let mut word = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err(ScriptError::Unterminated(start)),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some(&c) => {
                            if c == '\n' {
                                line += 1;
                            }
                            word.push(c);
                            i += 1;
                        }
                    }
                }
                out.push((Token::Word(word), line));
            }
            _ => {
                let mut word = String::new();
                while let Some(&c) = chars.get(i) {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                        break;
                    }
                    if c == '/' && matches!(chars.get(i + 1), Some('/') | Some('*')) {
                        break;
                    }
                    word.push(c);
                    i += 1;
                }
                out.push((Token::Word(word), line));
            }
        }
    }
    Ok(out)
}

/// Parses a whole file into a root block whose children are its top-level
/// blocks; words outside any block become the root's lines.
pub fn parse(bytes: &[u8]) -> Result<Block, ScriptError> {
    let text = String::from_utf8_lossy(bytes);
    let mut stack = vec![Block::default()];
    let mut line_words: Vec<String> = Vec::new();
    for (token, line) in tokens(&text)? {
        match token {
            Token::Word(word) => line_words.push(word),
            Token::Newline => {
                if !line_words.is_empty() {
                    let words = std::mem::take(&mut line_words);
                    stack.last_mut().expect("root").lines.push(words);
                }
            }
            Token::Open => {
                if stack.len() > MAX_DEPTH {
                    return Err(ScriptError::TooDeep(line));
                }
                // The block's name is the words on this line, or else the
                // last line of words before it.
                let name = if line_words.is_empty() {
                    stack
                        .last_mut()
                        .expect("root")
                        .lines
                        .pop()
                        .unwrap_or_else(Vec::new)
                } else {
                    std::mem::take(&mut line_words)
                };
                stack.push(Block {
                    name,
                    ..Block::default()
                });
            }
            Token::Close => {
                if !line_words.is_empty() {
                    let words = std::mem::take(&mut line_words);
                    stack.last_mut().expect("root").lines.push(words);
                }
                if stack.len() == 1 {
                    return Err(ScriptError::UnexpectedClose(line));
                }
                let block = stack.pop().expect("more than the root");
                stack.last_mut().expect("root").children.push(block);
            }
        }
    }
    if !line_words.is_empty() {
        stack.last_mut().expect("root").lines.push(line_words);
    }
    if stack.len() > 1 {
        return Err(ScriptError::Unclosed(stack.len() - 1));
    }
    Ok(stack.pop().expect("root"))
}
