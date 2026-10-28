//! 2DA V2.b: the binary tables behind most game rules (appearance, base
//! items, feats, …).
//!
//! Layout: `2DA V2.b\n`; column names, each followed by a separator, then a
//! NUL; a u32 row count; one label per row, each followed by a separator;
//! a u16 offset per cell (row by row) into the cell data; a u16 size of the
//! cell data; the cell data, NUL-terminated strings that cells may share.
//!
//! The separator is a tab in most files. Some copies (the 2DAs in K1's
//! `rims/global.rim`) use NUL instead; both are read, as the xoreos
//! reference does, and the file's own separator is kept for writing back.

use crate::FormatError;

const FORMAT: &str = "2DA";
const SIGNATURE: &[u8] = b"2DA V2.b\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwoDa {
    pub columns: Vec<Vec<u8>>,
    /// The label each row carries. Rows are addressed by position; the
    /// labels are usually `0`, `1`, … but are not relied on.
    pub row_labels: Vec<Vec<u8>>,
    /// `cells[row][column]`, raw bytes. `****` and the empty string both
    /// mean "no value"; values are read through `get` or `named`.
    pub cells: Vec<Vec<Vec<u8>>>,
    /// The byte after each column name: tab or NUL.
    pub column_separator: u8,
    /// The byte after each row label: tab or NUL.
    pub row_separator: u8,
}

fn truncated(needed: usize, len: usize) -> FormatError {
    FormatError::Truncated {
        format: FORMAT,
        needed,
        len,
    }
}

fn unsupported(what: String) -> FormatError {
    FormatError::Unsupported {
        format: FORMAT,
        what,
    }
}

impl TwoDa {
    pub fn parse(bytes: &[u8]) -> Result<TwoDa, FormatError> {
        if !bytes.starts_with(SIGNATURE) {
            let mut found = [0u8; 8];
            let len = bytes.len().min(8);
            found[..len].copy_from_slice(&bytes[..len]);
            return Err(FormatError::Signature {
                format: FORMAT,
                found,
            });
        }
        let mut at = SIGNATURE.len();

        // Column names: the list ends at an empty name.
        let mut columns = Vec::new();
        let mut column_separator = b'\t';
        loop {
            let end = bytes[at..]
                .iter()
                .position(|&b| b == b'\t' || b == 0)
                .map(|offset| at + offset)
                .ok_or(truncated(bytes.len() + 1, bytes.len()))?;
            if end == at {
                if bytes[end] != 0 {
                    return Err(unsupported(
                        "empty column name before the header ends".into(),
                    ));
                }
                at = end + 1;
                break;
            }
            columns.push(bytes[at..end].to_vec());
            column_separator = bytes[end];
            at = end + 1;
        }

        let count_bytes = bytes
            .get(at..at + 4)
            .ok_or(truncated(at + 4, bytes.len()))?;
        let rows = u32::from_le_bytes(count_bytes.try_into().expect("4 bytes")) as usize;
        at += 4;
        // Each label takes at least its separator.
        let mut row_labels = Vec::with_capacity(rows.min(bytes.len() - at));
        let mut row_separator = b'\t';
        for _ in 0..rows {
            let end = bytes[at..]
                .iter()
                .position(|&b| b == b'\t' || b == 0)
                .map(|offset| at + offset)
                .ok_or(truncated(bytes.len() + 1, bytes.len()))?;
            row_labels.push(bytes[at..end].to_vec());
            row_separator = bytes[end];
            at = end + 1;
        }

        let cell_count = rows * columns.len();
        let offsets_end = at + cell_count * 2;
        let table = bytes
            .get(at..offsets_end + 2)
            .ok_or(truncated(offsets_end + 2, bytes.len()))?;
        let data_size =
            u16::from_le_bytes([table[cell_count * 2], table[cell_count * 2 + 1]]) as usize;
        let data_at = offsets_end + 2;
        let data = bytes
            .get(data_at..data_at + data_size)
            .ok_or(truncated(data_at + data_size, bytes.len()))?;
        if data_at + data_size != bytes.len() {
            return Err(unsupported(format!(
                "{} bytes after the cell data",
                bytes.len() - data_at - data_size
            )));
        }
        let mut cells = Vec::with_capacity(rows);
        for row in 0..rows {
            let mut values = Vec::with_capacity(columns.len());
            for column in 0..columns.len() {
                let i = (row * columns.len() + column) * 2;
                let offset = u16::from_le_bytes([table[i], table[i + 1]]) as usize;
                let end = data
                    .get(offset..)
                    .and_then(|rest| rest.iter().position(|&b| b == 0))
                    .map(|len| offset + len)
                    .ok_or_else(|| unsupported(format!("cell offset {offset} of {data_size}")))?;
                values.push(data[offset..end].to_vec());
            }
            cells.push(values);
        }
        Ok(TwoDa {
            columns,
            row_labels,
            cells,
            column_separator,
            row_separator,
        })
    }

    /// Writes the table back. Cell strings are stored once each, in order of
    /// first use, row by row, as the games' own files do.
    pub fn write(&self) -> Vec<u8> {
        let mut out = SIGNATURE.to_vec();
        for column in &self.columns {
            out.extend_from_slice(column);
            out.push(self.column_separator);
        }
        out.push(0);
        out.extend_from_slice(&(self.row_labels.len() as u32).to_le_bytes());
        for label in &self.row_labels {
            out.extend_from_slice(label);
            out.push(self.row_separator);
        }
        let mut data: Vec<u8> = Vec::new();
        let mut known: Vec<(&[u8], u16)> = Vec::new();
        let mut offsets = Vec::with_capacity(self.cells.len() * self.columns.len());
        for row in &self.cells {
            for cell in row {
                let offset = match known.iter().find(|(text, _)| *text == cell.as_slice()) {
                    Some((_, offset)) => *offset,
                    None => {
                        let offset = data.len() as u16;
                        data.extend_from_slice(cell);
                        data.push(0);
                        known.push((cell, offset));
                        offset
                    }
                };
                offsets.push(offset);
            }
        }
        for offset in offsets {
            out.extend_from_slice(&offset.to_le_bytes());
        }
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case(name.as_bytes()))
    }

    /// A cell's text, or `None` for `****` or an empty cell (both mean "no
    /// value"; xoreos turns an empty cell into `****`; `crate::is_blank`).
    pub fn get(&self, row: usize, column: usize) -> Option<&[u8]> {
        let cell = self.cells.get(row)?.get(column)?;
        (!crate::is_blank(cell)).then_some(cell.as_slice())
    }

    /// The resource a cell names, lowercased (`crate::named`).
    pub fn named(&self, row: usize, column: usize) -> Option<String> {
        crate::named(self.get(row, column)?)
    }

    /// The cell of `row` under the column called `column`, trimmed and as
    /// written; `None` for a blank one (`get`) or no such column.
    pub fn text(&self, row: usize, column: &str) -> Option<String> {
        let cell = self.get(row, self.column(column)?)?;
        Some(String::from_utf8_lossy(cell).trim().to_string())
    }

    /// The resource the cell under `column` names, lowercased (`named`).
    pub fn resref(&self, row: usize, column: &str) -> Option<String> {
        self.named(row, self.column(column)?)
    }

    /// The cell under `column` as an integer: its leading digits, after
    /// blanks and a sign, as reone reads a 2DA number; `None`
    /// when blank or it starts with none.
    pub fn int(&self, row: usize, column: &str) -> Option<i64> {
        let text = self.text(row, column)?;
        let end = text
            .char_indices()
            .find(|&(at, c)| !(c.is_ascii_digit() || (at == 0 && (c == '-' || c == '+'))))
            .map_or(text.len(), |(at, _)| at);
        text[..end].parse().ok()
    }

    /// The cell under `column` as a number with a fraction.
    pub fn float(&self, row: usize, column: &str) -> Option<f32> {
        self.text(row, column)?.parse().ok()
    }

    /// The cell under `column` as reone reads a boolean: a non-zero integer.
    pub fn flag(&self, row: usize, column: &str) -> bool {
        self.int(row, column).is_some_and(|value| value != 0)
    }

    /// The cell under `column` as hexadecimal bits, `0x` before them or not
    /// (`baseitems.2da` `equipableslots`).
    pub fn hex(&self, row: usize, column: &str) -> Option<u32> {
        let text = self.text(row, column)?;
        let digits = text
            .strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))
            .unwrap_or(&text);
        u32::from_str_radix(digits, 16).ok()
    }

    /// The first row whose cell under `column` is `value`, ignoring case
    /// (a row by its `label`).
    pub fn find(&self, column: &str, value: &str) -> Option<usize> {
        let column = self.column(column)?;
        (0..self.cells.len()).find(|&row| {
            self.get(row, column)
                .is_some_and(|cell| cell.trim_ascii().eq_ignore_ascii_case(value.as_bytes()))
        })
    }
}
