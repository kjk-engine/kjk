//! A stereo 16-bit WAV file written as the mix is made: the header's
//! lengths are brought up to date after every append, so the file is
//! whole however the run ends.

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;

const HEADER: u32 = 44;

pub struct WavFile {
    file: File,
    rate: u32,
    /// Bytes of samples written.
    data: u32,
}

impl WavFile {
    pub fn create(path: &Path, rate: u32) -> io::Result<WavFile> {
        let mut wav = WavFile {
            file: File::create(path)?,
            rate,
            data: 0,
        };
        wav.header()?;
        Ok(wav)
    }

    /// Interleaved left and right samples, in −1 to 1.
    pub fn append(&mut self, samples: &[f32]) -> io::Result<()> {
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
            .collect();
        self.file
            .seek(SeekFrom::Start(u64::from(HEADER + self.data)))?;
        self.file.write_all(&bytes)?;
        self.data += bytes.len() as u32;
        self.header()
    }

    /// Seconds written.
    pub fn seconds(&self) -> f64 {
        f64::from(self.data) / f64::from(self.rate * 4)
    }

    fn header(&mut self) -> io::Result<()> {
        let mut header = Vec::with_capacity(HEADER as usize);
        header.extend(b"RIFF");
        header.extend((HEADER - 8 + self.data).to_le_bytes());
        header.extend(b"WAVEfmt ");
        header.extend(16u32.to_le_bytes());
        header.extend(1u16.to_le_bytes()); // PCM
        header.extend(2u16.to_le_bytes()); // stereo
        header.extend(self.rate.to_le_bytes());
        header.extend((self.rate * 4).to_le_bytes());
        header.extend(4u16.to_le_bytes());
        header.extend(16u16.to_le_bytes());
        header.extend(b"data");
        header.extend(self.data.to_le_bytes());
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header)
    }
}
