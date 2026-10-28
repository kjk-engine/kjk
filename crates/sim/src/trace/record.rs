//! The trace file: a header line (`#kjk-trace 1 <pkg>`), then for each
//! scenario a line `== <name>` and one record a step. A record is the
//! step's number and `key=value` words: floats as their bits in hex
//! (`f32::to_bits`, eight digits), vectors as such words joined by commas,
//! integers in decimal, enums by name, flags as numbers.

use std::fmt::Write as _;
use std::io::{BufWriter, Write};
use std::path::Path;

/// The format's version, in the header.
pub fn version() -> u32 {
    1
}

/// One step's record, built field by field.
pub struct Record {
    line: String,
}

impl Record {
    /// Step `step` of the scenario, doing `op`.
    pub fn new(step: u64, op: &str) -> Record {
        let mut line = String::with_capacity(512);
        let _ = write!(line, "{step} op={op}");
        Record { line }
    }

    fn key(&mut self, key: &str) {
        self.line.push(' ');
        self.line.push_str(key);
        self.line.push('=');
    }

    pub fn f(&mut self, key: &str, value: f32) -> &mut Record {
        self.key(key);
        let _ = write!(self.line, "{:08x}", value.to_bits());
        self
    }

    pub fn v3(&mut self, key: &str, value: [f32; 3]) -> &mut Record {
        self.key(key);
        let _ = write!(
            self.line,
            "{:08x},{:08x},{:08x}",
            value[0].to_bits(),
            value[1].to_bits(),
            value[2].to_bits()
        );
        self
    }

    pub fn fs(&mut self, key: &str, values: &[f32]) -> &mut Record {
        self.key(key);
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.line.push(',');
            }
            let _ = write!(self.line, "{:08x}", value.to_bits());
        }
        self
    }

    pub fn i(&mut self, key: &str, value: impl std::fmt::Display) -> &mut Record {
        self.key(key);
        let _ = write!(self.line, "{value}");
        self
    }

    /// Integers joined by commas (`-` for none).
    pub fn list<T: std::fmt::Display>(&mut self, key: &str, values: &[T]) -> &mut Record {
        self.key(key);
        if values.is_empty() {
            self.line.push('-');
        }
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.line.push(',');
            }
            let _ = write!(self.line, "{value}");
        }
        self
    }

    pub fn b(&mut self, key: &str, value: bool) -> &mut Record {
        self.key(key);
        self.line.push(if value { '1' } else { '0' });
        self
    }

    /// An enum or name (`Debug` for an enum).
    pub fn s(&mut self, key: &str, value: &str) -> &mut Record {
        self.key(key);
        self.line
            .push_str(if value.is_empty() { "-" } else { value });
        self
    }

    /// `Debug` of a value, spaces taken out (for enums and small
    /// structs of integers).
    pub fn d(&mut self, key: &str, value: &impl std::fmt::Debug) -> &mut Record {
        self.key(key);
        let text = format!("{value:?}");
        self.line
            .extend(text.chars().filter(|c| !c.is_whitespace()));
        self
    }

    /// Bytes as hex (an encoding, or a digest).
    pub fn hex(&mut self, key: &str, bytes: &[u8]) -> &mut Record {
        self.key(key);
        for byte in bytes {
            let _ = write!(self.line, "{byte:02x}");
        }
        if bytes.is_empty() {
            self.line.push('-');
        }
        self
    }
}

/// A trace being written.
pub struct Trace {
    out: BufWriter<std::fs::File>,
    path: String,
}

impl Trace {
    pub fn create(path: &Path, pkg: &str) -> Result<Trace, String> {
        let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut trace = Trace {
            out: BufWriter::with_capacity(1 << 20, file),
            path: path.display().to_string(),
        };
        trace.line(&format!("#kjk-trace {} {pkg}", version()))?;
        Ok(trace)
    }

    fn line(&mut self, line: &str) -> Result<(), String> {
        self.out
            .write_all(line.as_bytes())
            .and_then(|()| self.out.write_all(b"\n"))
            .map_err(|e| format!("{}: {e}", self.path))
    }

    pub fn begin(&mut self, scenario: &str) -> Result<(), String> {
        self.line(&format!("== {scenario}"))
    }

    pub fn put(&mut self, record: &Record) -> Result<(), String> {
        self.line(&record.line)
    }

    pub fn finish(mut self) -> Result<(), String> {
        self.out.flush().map_err(|e| format!("{}: {e}", self.path))
    }
}
