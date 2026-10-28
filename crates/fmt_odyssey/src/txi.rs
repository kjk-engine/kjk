//! TXI: plain-text texture settings, either a `.txi` resource or text after
//! a TPC's pixel data. One setting per line: a keyword, then its values.
//! A few keywords (`upperleftcoords`, `lowerrightcoords`) take a count and
//! that many following lines.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Txi {
    /// In file order; keywords lowercased, values as written.
    pub settings: Vec<(String, Vec<String>)>,
    /// Multi-line blocks: keyword and its lines, each split into words.
    pub blocks: Vec<(String, Vec<Vec<String>>)>,
}

const BLOCK_KEYWORDS: &[&str] = &["upperleftcoords", "lowerrightcoords"];

impl Txi {
    pub fn parse(bytes: &[u8]) -> Txi {
        let text = String::from_utf8_lossy(bytes);
        let mut lines = text
            .split(['\n', '\r', '\0'])
            .map(str::trim)
            .filter(|line| !line.is_empty());
        let mut txi = Txi::default();
        while let Some(line) = lines.next() {
            let mut words = line.split_whitespace();
            let Some(keyword) = words.next() else {
                continue;
            };
            let keyword = keyword.to_ascii_lowercase();
            let values: Vec<String> = words.map(str::to_string).collect();
            if BLOCK_KEYWORDS.contains(&keyword.as_str())
                && let Some(count) = values.first().and_then(|v| v.parse::<usize>().ok())
            {
                let rows = lines
                    .by_ref()
                    .take(count)
                    .map(|row| row.split_whitespace().map(str::to_string).collect())
                    .collect();
                txi.blocks.push((keyword, rows));
                continue;
            }
            txi.settings.push((keyword, values));
        }
        txi
    }

    /// The last value list for a keyword (later lines override earlier ones).
    pub fn get(&self, keyword: &str) -> Option<&[String]> {
        self.settings
            .iter()
            .rev()
            .find(|(key, _)| key == keyword)
            .map(|(_, values)| values.as_slice())
    }

    pub fn number(&self, keyword: &str) -> Option<f32> {
        self.get(keyword)?.first()?.parse().ok()
    }

    /// Frames of a flip-book texture: `proceduretype cycle` with `numx`,
    /// `numy` and `fps` all set.
    pub fn cycle_frames(&self) -> Option<(u32, u32)> {
        let procedure = self.get("proceduretype")?.first()?;
        if !procedure.eq_ignore_ascii_case("cycle") {
            return None;
        }
        let x = self.number("numx")? as u32;
        let y = self.number("numy")? as u32;
        let fps = self.number("fps")?;
        (x > 0 && y > 0 && fps > 0.0).then_some((x, y))
    }
}
