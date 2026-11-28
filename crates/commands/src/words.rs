//! Splitting a line into commands and words.
//!
//! A `;` or a new line ends a command, as in `--cmds` scripts. Spaces and
//! tabs separate words. Double quotes group words into one, and a `;`
//! inside quotes is part of the word. A quote that is never closed runs to
//! the end of the line. Control characters are dropped.

/// The commands of `line`, each as its words. Empty commands are left out.
pub fn split(line: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    // Whether `word` is a word even when empty (`""`).
    let mut started = false;
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            ';' | '\n' | '\r' if !quoted => {
                end_word(&mut word, &mut started, &mut words);
                if !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
            }
            c if c.is_whitespace() && !quoted => end_word(&mut word, &mut started, &mut words),
            c if c.is_control() => {}
            c => word.push(c),
        }
    }
    end_word(&mut word, &mut started, &mut words);
    if !words.is_empty() {
        commands.push(words);
    }
    commands
}

/// Ends the word being read, if there is one.
fn end_word(word: &mut String, started: &mut bool, words: &mut Vec<String>) {
    if *started || !word.is_empty() {
        words.push(std::mem::take(word));
    }
    *started = false;
}
