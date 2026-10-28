//! LTR V1.0: a letter table that makes random names (character creation's
//! Random: `humanm`, `humanf` for first names, `humanl` for last names).
//!
//! Layout: `LTR V1.0`, the letter count (28: `a` to `z`, `'`, `-`), then
//! letter sets of that many floats each for where a letter starts, sits in
//! the middle of and ends a name: one set for single letters, one for each
//! letter before (pairs), one for each two letters before (triples). Each
//! list is cumulative: a letter is the first whose value passes a random
//! draw.
//!
//! A name (`Ltr::name`, a rule xoreos and reone share): three letters by
//! the single, pair and triple start lists, the first upper case; then
//! from 0 to `most - 3` letters by the triples' middle lists; then one by
//! their end list.

use crate::FormatError;
use crate::reader::Reader;

const FORMAT: &str = "LTR";
/// `a` to `z`, the apostrophe and the hyphen.
const LETTERS: &[u8; 28] = b"abcdefghijklmnopqrstuvwxyz'-";
const HEADER_LEN: usize = 9;

/// Where a letter may come: its cumulative chances at the start, in the
/// middle and at the end of a name.
#[derive(Debug, Clone, PartialEq)]
pub struct LetterSet {
    pub start: Vec<f32>,
    pub mid: Vec<f32>,
    pub end: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ltr {
    pub single: LetterSet,
    /// By the letter before.
    pub pairs: Vec<LetterSet>,
    /// By the two letters before.
    pub triples: Vec<Vec<LetterSet>>,
}

impl Ltr {
    pub fn parse(bytes: &[u8]) -> Result<Ltr, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        reader.expect_tag(b"LTR V1.0")?;
        let count = usize::from(*reader.slice(8, 1)?.first().unwrap_or(&0));
        if count != LETTERS.len() {
            return Err(FormatError::Unsupported {
                format: FORMAT,
                what: format!("{count} letters"),
            });
        }
        let mut at = HEADER_LEN;
        let mut set = || -> Result<LetterSet, FormatError> {
            let mut list = || -> Result<Vec<f32>, FormatError> {
                let floats = (0..count)
                    .map(|n| reader.u32(at + 4 * n).map(f32::from_bits))
                    .collect();
                at += 4 * count;
                floats
            };
            Ok(LetterSet {
                start: list()?,
                mid: list()?,
                end: list()?,
            })
        };
        let single = set()?;
        let pairs = (0..count).map(|_| set()).collect::<Result<_, _>>()?;
        let triples = (0..count)
            .map(|_| (0..count).map(|_| set()).collect())
            .collect::<Result<_, _>>()?;
        Ok(Ltr {
            single,
            pairs,
            triples,
        })
    }

    /// A random name of at most `most` letters (4 or more), `draw` giving
    /// numbers from 0 to 1.
    pub fn name(&self, most: usize, draw: &mut dyn FnMut() -> f32) -> String {
        let pick = |list: &[f32], chance: f32| list.iter().position(|&passed| passed > chance);
        let mut name = String::new();
        let (mut before, mut last) = (0, 0);
        if let Some(first) = pick(&self.single.start, draw()) {
            name.push(char::from(LETTERS[first].to_ascii_uppercase()));
            before = first;
        }
        if let Some(second) = pick(&self.pairs[before].start, draw()) {
            name.push(char::from(LETTERS[second]));
            last = second;
        }
        let middle = most.saturating_sub(3);
        let between = ((draw() * (middle + 1) as f32) as usize).min(middle);
        let lists = std::iter::once(Place::Start)
            .chain(std::iter::repeat_n(Place::Middle, between))
            .chain(std::iter::once(Place::End));
        for place in lists {
            let set = &self.triples[before][last];
            let list = match place {
                Place::Start => &set.start,
                Place::Middle => &set.mid,
                Place::End => &set.end,
            };
            if let Some(letter) = pick(list, draw()) {
                name.push(char::from(LETTERS[letter]));
                (before, last) = (last, letter);
            }
        }
        name
    }
}

/// Where in a name a letter goes.
#[derive(Clone, Copy)]
enum Place {
    Start,
    Middle,
    End,
}
