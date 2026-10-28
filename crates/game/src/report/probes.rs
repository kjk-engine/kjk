//! The probes of `report` (`kjk sim probes`, group "report").

use super::*;
use sim::probe::{Checks, check};

/// A Windows home and two others made from parts (a home with a space, a
/// name with letters past ASCII), so no personal path is written here.
fn homes() -> [(String, String); 3] {
    let windows = format!("C:{}Users{}Åsa", '\\', '\\');
    let linux = ["", "home", "zoë"].join("/");
    let mac = ["", "Users", "a b"].join("/");
    [(windows, "Åsa".into()), (linux, "zoë".into()), (mac, "a b".into())]
}

pub fn probes() -> Checks {
    let root = std::env::temp_dir().join(format!("kjk-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let checks = vec![
        ("scrub_homes", scrubs()),
        ("scrub_host", host()),
        ("zip_has_no_identity", no_identity()),
        ("settings_drop_host_id", drop_host_id()),
        ("zip_cap_drops_oldest_flight", cap()),
        ("reports_keep_20", keep(&root)),
    ];
    let _ = std::fs::remove_dir_all(&root);
    checks
}

fn scrubs() -> Result<String, String> {
    let wrong: Vec<String> = homes()
        .iter()
        .flat_map(|(home, user)| {
            let lower = home.to_lowercase().replace('\\', "/");
            [
                (format!("{home}{}kjk{}saves", SEP[0], SEP[0]), format!("~{}kjk{}saves", SEP[0], SEP[0])),
                (format!("{lower}/x"), "~/x".to_string()),
                (format!("{user} was here"), "<user> was here".to_string()),
            ]
            .map(|(text, wanted)| (scrub(&text, home, user, "box-7"), wanted))
        })
        .filter(|(got, wanted)| got != wanted)
        .map(|(got, wanted)| format!("{got:?} != {wanted:?}"))
        .collect();
    check(wrong.is_empty(), format!("{wrong:?}"))
}

/// The two ways a path's separator is written, Windows' first.
const SEP: [char; 2] = ['\\', '/'];

fn host() -> Result<String, String> {
    let got = scrub("user zoë on box-7", "/nowhere", "zoë", "box-7");
    check(got == "user <user> on <host>", got)
}

fn part(name: &str, text: &str) -> Part {
    Part {
        name: name.into(),
        bytes: text.as_bytes().to_vec(),
        text: true,
    }
}

/// Whether `zip`'s files hold `needle` anywhere, compressed or not.
fn holds(zip: &[u8], needle: &str) -> Result<bool, String> {
    let mut archive = ::zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes).map_err(|error| error.to_string())?;
        if bytes.windows(needle.len()).any(|window| window == needle.as_bytes()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn no_identity() -> Result<String, String> {
    let (home, user) = homes()[1].clone();
    let me = Identity {
        home: home.clone(),
        user: user.clone(),
        host: "box-7".into(),
    };
    let parts = vec![
        part("kjk.log", &format!("run folder: {home}/.config/kjk/logs on box-7 for {user}")),
        part("settings.json", &format!("{{\"saves\":\"{home}/saves\"}}")),
    ];
    let (zipped, _) = zip(parts, &me, LIMIT);
    let found: Vec<&str> = [home.as_str(), user.as_str(), "box-7"]
        .into_iter()
        .filter(|needle| holds(&zipped, needle).unwrap_or(true))
        .collect();
    check(found.is_empty(), format!("found {found:?}"))
}

fn drop_host_id() -> Result<String, String> {
    let kept = settings_text(r#"{"host.id":"0011223344556677","sound.music":"0.5"}"#);
    check(!kept.contains("host.id") && kept.contains("sound.music"), kept)
}

fn cap() -> Result<String, String> {
    // 10 MiB that do not deflate (xorshift).
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let ten: Vec<u8> = (0..10 * 1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let flight = |n: u32| Part {
        name: format!("flight-{n}.kjkf"),
        bytes: ten.clone(),
        text: false,
    };
    let me = Identity::default();
    let (zipped, dropped) = zip(vec![part("note.txt", "hi"), flight(1), flight(2), flight(3)], &me, 25 * 1024 * 1024);
    check(
        dropped == ["flight-1.kjkf"] && zipped.len() <= 25 * 1024 * 1024,
        format!("dropped {dropped:?}, {} bytes", zipped.len()),
    )
}

fn keep(root: &Path) -> Result<String, String> {
    for n in 0..23 {
        save(root, format!("report {n}").as_bytes()).map_err(|error| error.to_string())?;
    }
    let count = std::fs::read_dir(root).map_or(0, |entries| entries.count());
    check(count == KEPT, format!("{count} kept"))
}
