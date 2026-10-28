//! Bytes for a built level and navigation mesh kept in the store
//! (`docs/IMPORT.md`, slice 3): little-endian, every float by its bits, so
//! a level read back is the level written, bit for bit (its digest too).

/// A failed read: the bytes end early or say something impossible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bad(pub &'static str);

impl std::fmt::Display for Bad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a stored collision bake does not read: {}", self.0)
    }
}

impl std::error::Error for Bad {}

#[derive(Default)]
pub(crate) struct Out(pub Vec<u8>);

impl Out {
    pub fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    pub fn u16(&mut self, value: u16) {
        self.0.extend(value.to_le_bytes());
    }

    pub fn u32(&mut self, value: u32) {
        self.0.extend(value.to_le_bytes());
    }

    pub fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    pub fn vec3(&mut self, value: [f32; 3]) {
        for axis in value {
            self.f32(axis);
        }
    }

    pub fn len(&mut self, len: usize) {
        self.u32(u32::try_from(len).expect("fewer than 2^32 things"));
    }
}

pub(crate) struct In<'a>(pub &'a [u8]);

impl In<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Bad> {
        let (head, rest) = self.0.split_first_chunk::<N>().ok_or(Bad("cut short"))?;
        self.0 = rest;
        Ok(*head)
    }

    pub fn u8(&mut self) -> Result<u8, Bad> {
        Ok(self.take::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16, Bad> {
        self.take().map(u16::from_le_bytes)
    }

    pub fn u32(&mut self) -> Result<u32, Bad> {
        self.take().map(u32::from_le_bytes)
    }

    pub fn i32(&mut self) -> Result<i32, Bad> {
        self.take().map(i32::from_le_bytes)
    }

    pub fn f32(&mut self) -> Result<f32, Bad> {
        self.u32().map(f32::from_bits)
    }

    pub fn vec3(&mut self) -> Result<[f32; 3], Bad> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    /// A count of things, each at least `least` bytes: more than the bytes
    /// left could hold is refused before anything is allocated.
    pub fn len(&mut self, least: usize) -> Result<usize, Bad> {
        let len = self.u32()? as usize;
        if len.saturating_mul(least.max(1)) > self.0.len() {
            return Err(Bad("a count past the end"));
        }
        Ok(len)
    }

    pub fn magic(&mut self, magic: &[u8; 4], version: u32) -> Result<(), Bad> {
        if &self.take::<4>()? != magic || self.u32()? != version {
            return Err(Bad("another format or version"));
        }
        Ok(())
    }

    pub fn finish(&self) -> Result<(), Bad> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Bad("bytes left over"))
        }
    }
}
