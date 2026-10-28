//! BLAKE3 fed through a buffer, for digests made of many small values.

/// The same digest as `blake3::Hasher` given the same bytes (BLAKE3's
/// result does not depend on how its input is split), but tens of
/// thousands of four-byte values go in as a few large pieces instead of one
/// call each: the Endar Spire's collision digest took 71 ms that way.
pub struct Digester {
    hasher: blake3::Hasher,
    buffer: Vec<u8>,
}

/// How much is gathered before it is hashed.
const PIECE: usize = 1 << 16;

impl Default for Digester {
    fn default() -> Digester {
        Digester {
            hasher: blake3::Hasher::new(),
            buffer: Vec::with_capacity(PIECE),
        }
    }
}

impl Digester {
    pub fn new() -> Digester {
        Digester::default()
    }

    pub fn update(&mut self, bytes: &[u8]) -> &mut Digester {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() >= PIECE {
            self.hasher.update(&self.buffer);
            self.buffer.clear();
        }
        self
    }

    pub fn finalize(&mut self) -> blake3::Hash {
        self.hasher.update(&self.buffer);
        self.buffer.clear();
        self.hasher.finalize()
    }
}
