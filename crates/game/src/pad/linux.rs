//! Valve's pads on Linux, mapped as the kernel reports them. SDL's database
//! (which gilrs carries) maps the Steam Deck's own controls by the buttons
//! the kernel listed when the entry was written: a kernel that lists fewer
//! (no trackpad clicks, no Quick Access) shifts every button, Y reading as A
//! and L1 as B, and the sticks with them. The kernel's driver names its
//! buttons and axes as Linux's gamepad API does (`BTN_SOUTH`, `ABS_X`), so a
//! mapping made from what this machine's kernel lists, in gilrs' own order,
//! is right whatever the kernel; `SDL_GAMECONTROLLERCONFIG` (which gilrs
//! reads before its database) carries it.

/// Valve's USB vendor id.
const VALVE: u16 = 0x28de;
/// Steam's own virtual pad (Steam Input's), which SDL's database maps
/// right, its D-pad a hat: left to it.
const STEAM_VIRTUAL: u16 = 0x11ff;

/// A pad's buttons by their Linux code, as SDL names them.
const BUTTONS: [(u16, &str); 15] = [
    (0x130, "a"),
    (0x131, "b"),
    (0x133, "x"),
    (0x134, "y"),
    (0x136, "leftshoulder"),
    (0x137, "rightshoulder"),
    (0x13a, "back"),
    (0x13b, "start"),
    (0x13c, "guide"),
    (0x13d, "leftstick"),
    (0x13e, "rightstick"),
    (0x220, "dpup"),
    (0x221, "dpdown"),
    (0x222, "dpleft"),
    (0x223, "dpright"),
];

/// Its sticks by their Linux code.
const STICKS: [(u16, &str); 4] = [
    (0x00, "leftx"),
    (0x01, "lefty"),
    (0x03, "rightx"),
    (0x04, "righty"),
];

/// Its triggers, left and right: as axes (`ABS_Z`/`ABS_RZ`, the Deck's
/// `ABS_HAT2Y`/`ABS_HAT2X`), else as buttons (`BTN_TL2`/`BTN_TR2`).
const TRIGGER_AXES: [[u16; 2]; 2] = [[0x02, 0x05], [0x15, 0x14]];
const TRIGGER_BUTTONS: [u16; 2] = [0x138, 0x139];

/// A Linux input device as its `/sys` entry describes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    pub name: String,
    /// Bus, vendor, product and version (`/sys/…/id`).
    pub ids: [u16; 4],
    /// Its key and absolute axis codes.
    pub keys: Vec<u16>,
    pub axes: Vec<u16>,
}

/// The codes a `/sys` capability mask lists: words of hex, the highest
/// first, each 64 bits.
pub fn codes(mask: &str) -> Vec<u16> {
    let mut codes = Vec::new();
    for (word, hex) in mask.split_whitespace().rev().enumerate() {
        let bits = u64::from_str_radix(hex, 16).unwrap_or(0);
        for bit in 0..64 {
            if bits & (1 << bit) != 0 {
                codes.push((word * 64 + bit) as u16);
            }
        }
    }
    codes
}

/// The device's buttons in gilrs' order (`gilrs-core`'s Linux
/// `find_buttons`): `BTN_MISC` up to `BTN_MOUSE`, then `BTN_JOYSTICK` up,
/// then the keyboard's and the mouse's.
fn button_order(keys: &[u16]) -> Vec<u16> {
    let mut sorted = keys.to_vec();
    sorted.sort_unstable();
    let band = |from: u16, to: u16| {
        sorted
            .iter()
            .copied()
            .filter(move |code| (from..to).contains(code))
    };
    band(0x100, 0x110)
        .chain(band(0x120, u16::MAX))
        .chain(band(0, 0x100))
        .chain(band(0x110, 0x120))
        .collect()
}

/// SDL's id for the device (as gilrs reads its database by it).
fn guid([bus, vendor, product, version]: [u16; 4]) -> String {
    [bus, vendor, product, version]
        .iter()
        .map(|id| format!("{:02x}{:02x}0000", id & 0xff, id >> 8))
        .collect()
}

/// The device's mapping as SDL writes it, from the codes it lists; none for
/// a device that is no pad of Valve's (or lists no A), or is Steam's
/// virtual pad.
pub fn mapping(device: &Device) -> Option<String> {
    if device.ids[1] != VALVE || device.ids[2] == STEAM_VIRTUAL || !device.keys.contains(&0x130) {
        return None;
    }
    let buttons = button_order(&device.keys);
    let mut axes = device.axes.clone();
    axes.sort_unstable();
    let button = |code: u16| buttons.iter().position(|b| *b == code);
    let axis = |code: u16| axes.iter().position(|a| *a == code);
    let mut parts = vec![guid(device.ids), device.name.replace(',', " ")];
    for (code, name) in BUTTONS {
        if let Some(at) = button(code) {
            parts.push(format!("{name}:b{at}"));
        }
    }
    for (code, name) in STICKS {
        if let Some(at) = axis(code) {
            parts.push(format!("{name}:a{at}"));
        }
    }
    let triggers = TRIGGER_AXES
        .iter()
        .find_map(|[left, right]| {
            Some(format!(
                "lefttrigger:a{},righttrigger:a{}",
                axis(*left)?,
                axis(*right)?
            ))
        })
        .or_else(|| {
            let [left, right] = TRIGGER_BUTTONS;
            Some(format!(
                "lefttrigger:b{},righttrigger:b{}",
                button(left)?,
                button(right)?
            ))
        });
    parts.extend(triggers);
    parts.push("platform:Linux".into());
    Some(parts.join(",") + ",")
}

/// The mappings of Valve's pads plugged in now (`/sys/class/input`), one
/// a line; none where there are none.
#[cfg(target_os = "linux")]
pub fn valve_mappings() -> Option<String> {
    let read = |path: std::path::PathBuf| std::fs::read_to_string(path).ok();
    let hex =
        |text: Option<String>| text.and_then(|text| u16::from_str_radix(text.trim(), 16).ok());
    let lines: Vec<String> = std::fs::read_dir("/sys/class/input")
        .ok()?
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("event"))
        .filter_map(|entry| {
            let device = entry.path().join("device");
            let id = |name: &str| hex(read(device.join("id").join(name)));
            let found = Device {
                name: read(device.join("name"))?.trim().to_string(),
                ids: [
                    id("bustype")?,
                    id("vendor")?,
                    id("product")?,
                    id("version")?,
                ],
                keys: codes(&read(device.join("capabilities/key"))?),
                axes: codes(&read(device.join("capabilities/abs"))?),
            };
            mapping(&found)
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

#[cfg(feature = "probes")]
pub(super) fn probes() -> Vec<(&'static str, Result<String, String>)> {
    use sim::probe::check;
    // The Deck's kernel driver (`hid-steam`): its buttons, sticks, pads and
    // triggers; a newer kernel adds the trackpad clicks and Quick Access.
    let keys = [
        0x130, 0x131, 0x133, 0x134, 0x136, 0x137, 0x138, 0x139, 0x13a, 0x13b, 0x13c, 0x13d, 0x13e,
        0x220, 0x221, 0x222, 0x223, 0x2c0, 0x2c1, 0x2c2, 0x2c3,
    ];
    let axes = vec![0x00, 0x01, 0x03, 0x04, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15];
    let deck = |extra: &[u16], version: u16| Device {
        name: "Steam Deck".into(),
        ids: [0x0003, VALVE, 0x1205, version],
        keys: keys.iter().chain(extra).copied().collect(),
        axes: axes.clone(),
    };
    let older = mapping(&deck(&[], 0x0111)).unwrap_or_else(String::new);
    let newer = mapping(&deck(&[0x121, 0x122, 0x126], 0x0111)).unwrap_or_else(String::new);
    let others = mapping(&Device {
        ids: [0x0003, 0x045e, 0x028e, 0x0114],
        ..deck(&[], 0)
    })
    .or_else(|| {
        mapping(&Device {
            ids: [0x0003, VALVE, STEAM_VIRTUAL, 0x0001],
            ..deck(&[], 0)
        })
    });
    let mask = codes("f00000000 0 0 0 7fdb000000000000 0 0 0 0");
    vec![
        (
            "deck mapping",
            check(
                older.starts_with("03000000de2800000512000011010000,Steam Deck,a:b0,b:b1,x:b2,y:b3,leftshoulder:b4")
                    && older.contains("leftx:a0,lefty:a1,rightx:a2,righty:a3")
                    && older.contains("lefttrigger:a9,righttrigger:a8")
                    && older.ends_with("platform:Linux,"),
                older.clone(),
            ),
        ),
        (
            "deck newer kernel",
            check(newer.contains(",a:b3,b:b4,x:b5,y:b6,"), newer.clone()),
        ),
        (
            "only valve's own",
            check(others.is_none(), format!("{others:?}")),
        ),
        (
            "capability mask",
            check(
                mask.first() == Some(&0x130) && mask.contains(&0x13e) && mask.contains(&0x220),
                format!("{} codes from {:x?}", mask.len(), &mask[..mask.len().min(4)]),
            ),
        ),
    ]
}
