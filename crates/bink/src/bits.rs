//! Bink reads its bits least significant first, from 32-bit little-endian
//! words.

/// A reader over one packet's bits. Reading past the end gives zeros, and
/// `overrun` then says so.
pub struct Bits<'a> {
    bytes: &'a [u8],
    /// The position, in bits from the packet's start.
    at: usize,
}

impl<'a> Bits<'a> {
    pub fn new(bytes: &'a [u8]) -> Bits<'a> {
        Bits { bytes, at: 0 }
    }

    /// The next `count` bits (at most 32) without moving past them.
    pub fn peek(&self, count: u32) -> u32 {
        debug_assert!(count <= 32);
        let byte = self.at / 8;
        let mut word = 0u64;
        for i in 0..5 {
            let value = self.bytes.get(byte + i).copied().unwrap_or(0);
            word |= u64::from(value) << (8 * i);
        }
        let value = word >> (self.at % 8);
        (value & ((1u64 << count) - 1)) as u32
    }

    /// The next `count` bits (at most 32).
    pub fn read(&mut self, count: u32) -> u32 {
        let value = self.peek(count);
        self.at += count as usize;
        value
    }

    pub fn bit(&mut self) -> bool {
        self.read(1) == 1
    }

    pub fn skip(&mut self, count: usize) {
        self.at += count;
    }

    /// Moves to the next multiple of 32 bits.
    pub fn align32(&mut self) {
        self.at = self.at.next_multiple_of(32);
    }

    /// How many bits are left (negative past the end).
    pub fn left(&self) -> isize {
        (self.bytes.len() * 8) as isize - self.at as isize
    }

    /// Whether more bits were read than the packet has.
    pub fn overrun(&self) -> bool {
        self.left() < 0
    }
}
