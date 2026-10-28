//! `animation.cfg`: one line per animation sequence of a GLA:
//! `name first_frame frame_count loop_frame frames_per_second`. A loop frame
//! of −1 means the sequence does not loop; a negative speed plays it
//! backwards. `//` starts a comment.

#[derive(Debug, Clone, PartialEq)]
pub struct Sequence {
    pub name: String,
    pub first_frame: i32,
    pub frame_count: i32,
    pub loop_frame: i32,
    pub frames_per_second: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimationConfig {
    pub sequences: Vec<Sequence>,
    /// Lines that are not a name and four integers.
    pub bad_lines: Vec<String>,
}

impl AnimationConfig {
    pub fn parse(bytes: &[u8]) -> AnimationConfig {
        let text = String::from_utf8_lossy(bytes);
        let mut config = AnimationConfig::default();
        for line in text.lines() {
            let line = line.split("//").next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            let numbers: Option<Vec<i32>> = words
                .get(1..5)
                .map(|rest| rest.iter().filter_map(|w| w.parse().ok()).collect());
            match (words.first(), numbers) {
                (Some(name), Some(n)) if n.len() == 4 => config.sequences.push(Sequence {
                    name: name.to_string(),
                    first_frame: n[0],
                    frame_count: n[1],
                    loop_frame: n[2],
                    frames_per_second: n[3],
                }),
                _ => config.bad_lines.push(line.to_string()),
            }
        }
        config
    }
}
