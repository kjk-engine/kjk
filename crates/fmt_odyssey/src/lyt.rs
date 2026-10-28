//! LYT: an area's layout as text, between `beginlayout` and `donelayout`.
//! Each section is a keyword with a count, followed by that many lines:
//! `roomcount` (model x y z), `trackcount` and `obstaclecount` (model x y z),
//! `artplaceablecount` (model x y z), `doorhookcount` (room name x y z and
//! five more numbers) and `walkmeshRooms` (room). A `filedependancy` line
//! (BioWare's spelling) names the 3ds Max file the layout came from. Room
//! names are lowercased and kept as written: a room named `****` is one the
//! layout leaves out (`crate::is_blank`), as K1's stunt areas do.

use crate::FormatError;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub rooms: Vec<Placed>,
    pub tracks: Vec<Placed>,
    pub obstacles: Vec<Placed>,
    pub art_placeables: Vec<Placed>,
    pub door_hooks: Vec<DoorHook>,
    pub file_dependency: Option<String>,
    /// Lines outside every known section. The xoreos reference refuses a
    /// file with one; reone skips them. Kept so that reports can count them.
    pub unknown: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub model: String,
    pub position: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct DoorHook {
    pub room: String,
    pub name: String,
    pub position: [f32; 3],
    /// Five more numbers per hook; neither reference names them.
    pub extra: [f32; 5],
}

fn unsupported(what: String) -> FormatError {
    FormatError::Unsupported {
        format: "LYT",
        what,
    }
}

fn number(token: &str) -> Result<f32, FormatError> {
    token
        .parse()
        .map_err(|_| unsupported(format!("{token:?} is not a number")))
}

fn placed(words: &[&str], section: &str) -> Result<Placed, FormatError> {
    match words {
        [model, x, y, z, ..] => Ok(Placed {
            model: model.to_ascii_lowercase(),
            position: [number(x)?, number(y)?, number(z)?],
        }),
        _ => Err(unsupported(format!("{section} line {words:?}"))),
    }
}

impl Layout {
    pub fn parse(bytes: &[u8]) -> Result<Layout, FormatError> {
        let text = String::from_utf8_lossy(bytes);
        let mut lines = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'));
        let mut layout = Layout::default();
        while let Some(line) = lines.next() {
            let words: Vec<&str> = line.split_whitespace().collect();
            let keyword = words[0].to_ascii_lowercase();
            let count = |words: &[&str]| -> Result<usize, FormatError> {
                words
                    .get(1)
                    .and_then(|count| count.parse().ok())
                    .ok_or_else(|| unsupported(format!("{keyword} without a count")))
            };
            match keyword.as_str() {
                "beginlayout" => {}
                "filedependancy" => {
                    layout.file_dependency = words.get(1).map(|name| name.to_string())
                }
                "donelayout" => break,
                "roomcount" | "trackcount" | "obstaclecount" | "artplaceablecount"
                | "doorhookcount" | "walkmeshrooms" => {
                    let count = count(&words)?;
                    for _ in 0..count {
                        let entry = lines
                            .next()
                            .ok_or_else(|| unsupported(format!("{keyword} ends early")))?;
                        let entry: Vec<&str> = entry.split_whitespace().collect();
                        match keyword.as_str() {
                            "roomcount" => layout.rooms.push(placed(&entry, "room")?),
                            "trackcount" => layout.tracks.push(placed(&entry, "track")?),
                            "obstaclecount" => layout.obstacles.push(placed(&entry, "obstacle")?),
                            "artplaceablecount" => {
                                layout.art_placeables.push(placed(&entry, "art placeable")?)
                            }
                            "doorhookcount" => {
                                let [room, name, rest @ ..] = entry.as_slice() else {
                                    return Err(unsupported(format!("door hook {entry:?}")));
                                };
                                if rest.len() < 8 {
                                    return Err(unsupported(format!("door hook {entry:?}")));
                                }
                                let values: Vec<f32> = rest[..8]
                                    .iter()
                                    .map(|v| number(v))
                                    .collect::<Result<_, _>>()?;
                                layout.door_hooks.push(DoorHook {
                                    room: room.to_ascii_lowercase(),
                                    name: name.to_ascii_lowercase(),
                                    position: [values[0], values[1], values[2]],
                                    extra: [values[3], values[4], values[5], values[6], values[7]],
                                });
                            }
                            _ => {}
                        }
                    }
                }
                _ => layout.unknown.push(line.to_string()),
            }
        }
        Ok(layout)
    }
}
