//! Odyssey (KOTOR 1 and TSL) file formats. Pure parsers over `&[u8]`: this
//! crate opens no files.

pub mod bif;
pub mod bwm;
pub mod dlg;
pub mod dxt;
pub mod erf;
mod error;
pub mod gff;
pub mod gff_audit;
mod gff_write;
pub mod gui;
pub mod key;
pub mod lip;
pub mod ltr;
pub mod lyt;
pub mod mdl;
pub mod ncs;
mod reader;
mod restype;
pub mod rim;
pub mod ssf;
pub mod texture;
pub mod tlk;
pub mod twoda;
pub mod txi;
pub mod vis;
pub mod wav;

pub use error::FormatError;
pub use restype::ResType;

/// One resource in an ERF-family or RIM archive.
#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub resref: String,
    pub restype: ResType,
    /// Data offset from the start of the archive.
    pub offset: u32,
    pub size: u32,
    /// Bytes the format leaves unused next to the type. Zero in every
    /// well-formed archive; kept so a report can count the exceptions.
    pub reserved: u32,
}

/// A resource name: up to 16 bytes, NUL-padded on disk, compared without
/// case. Stored lowercased.
pub fn resref(raw: &[u8]) -> String {
    let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase()
}

/// Text in the games' code page (one character a byte) as a `String`.
pub fn code_page(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

/// Whether a field of the games' text tables holds nothing: empty, or
/// `****`, which a 2DA writes for an empty cell and a layout for a room it
/// leaves out. Surrounding blanks do not count.
pub fn is_blank(text: &[u8]) -> bool {
    let text = text.trim_ascii();
    text.is_empty() || text == b"****"
}

/// The resource a table cell or layout line names, lowercased; `None` when
/// it names none (`is_blank`).
pub fn named(text: &[u8]) -> Option<String> {
    (!is_blank(text)).then(|| String::from_utf8_lossy(text.trim_ascii()).to_ascii_lowercase())
}
