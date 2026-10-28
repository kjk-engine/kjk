//! The Windows C library's random numbers, which the games' executables draw
//! their grass, weather and lightning from. The same seed gives the same
//! places, so a room's grass is the one the game plants.

/// The Windows C library's `rand`, which the executables' grass, weather and
/// lightning draw from.
#[derive(Debug, Clone)]
pub struct MsvcRand(pub u32);

impl MsvcRand {
    /// 0 to 32767. Named as the C library names it; this is no iterator.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.0 >> 16) & 0x7fff
    }

    /// `next() % 10000 / 10000`: 0 to 0.9999.
    pub fn unit(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
}
