//! LIP V1.0: the mouth shapes of one voiced line over time.
//!
//! Layout: `LIP V1.0`, the line's length (f32 seconds), a key count (u32),
//! then that many keys of a time (f32 seconds) and a shape (u8). Shapes
//! index the 16 poses of a head's `talk` animation: 0 rest, 1 IH IY, 2 EH
//! ER EY, 3 AA AE AH, 4 OW OY, 5 UH UW W, 6 D DH S Y Z, 7 CH JH SH ZH, 8 F
//! V, 9 G HH K NG, 10 T TH, 11 B M P, 12 L N, 13 R, 14 AW AY, 15 AO.

use crate::FormatError;
use crate::reader::Reader;

const FORMAT: &str = "LIP";
const HEADER_LEN: usize = 16;
const KEY_LEN: usize = 5;
pub const SHAPES: u8 = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct Lip {
    pub length: f32,
    /// (seconds, shape), in file order.
    pub keys: Vec<(f32, u8)>,
}

impl Lip {
    pub fn parse(bytes: &[u8]) -> Result<Lip, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        reader.expect_tag(b"LIP V1.0")?;
        let length = f32::from_bits(reader.u32(8)?);
        let count = reader.u32(12)? as usize;
        reader.table(HEADER_LEN, count, KEY_LEN)?;
        let keys = (0..count)
            .map(|i| {
                let at = HEADER_LEN + i * KEY_LEN;
                Ok((f32::from_bits(reader.u32(at)?), reader.slice(at + 4, 1)?[0]))
            })
            .collect::<Result<_, FormatError>>()?;
        Ok(Lip { length, keys })
    }

    /// The two shapes around `at` seconds and how far from the first to
    /// the second (0 to 1), held at the first and last keys.
    pub fn shapes_at(&self, at: f32) -> (u8, u8, f32) {
        let after = self.keys.partition_point(|&(time, _)| time <= at);
        match (
            after.checked_sub(1).map(|i| self.keys[i]),
            self.keys.get(after),
        ) {
            (None, None) => (0, 0, 0.0),
            (None, Some(&(_, shape))) | (Some((_, shape)), None) => (shape, shape, 0.0),
            (Some((from, a)), Some(&(to, b))) => {
                let span = to - from;
                let blend = if span > 0.0 { (at - from) / span } else { 0.0 };
                (a, b, blend.clamp(0.0, 1.0))
            }
        }
    }
}
