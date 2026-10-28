//! VIS: which rooms can be seen from which, as text. A line with a room
//! name and a count starts a room; the following one-word lines are the
//! rooms visible from it. Names are lowercased. This is how both references
//! read it.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Visibility {
    /// (room, rooms visible from it) in file order.
    pub rooms: Vec<(String, Vec<String>)>,
    /// Rooms whose count differs from the lines that follow.
    pub count_mismatches: Vec<String>,
}

impl Visibility {
    pub fn parse(bytes: &[u8]) -> Visibility {
        let text = String::from_utf8_lossy(bytes);
        let mut vis = Visibility::default();
        let mut counts: Vec<usize> = Vec::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let words: Vec<&str> = line.split_whitespace().collect();
            if words.len() == 2 {
                vis.rooms.push((words[0].to_ascii_lowercase(), Vec::new()));
                counts.push(words[1].parse().unwrap_or(usize::MAX));
            } else if let Some((_, seen)) = vis.rooms.last_mut() {
                seen.push(words[0].to_ascii_lowercase());
            }
        }
        for ((room, seen), count) in vis.rooms.iter().zip(counts) {
            if seen.len() != count {
                vis.count_mismatches.push(room.clone());
            }
        }
        vis
    }

    pub fn visible_from(&self, room: &str) -> Option<&[String]> {
        self.rooms
            .iter()
            .find(|(name, _)| name == room)
            .map(|(_, seen)| seen.as_slice())
    }
}
