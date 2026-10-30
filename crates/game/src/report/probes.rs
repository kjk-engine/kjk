//! The probes of `report` (`kjk sim probes`, group "report").

use super::*;
use sim::probe::{Checks, check};

/// A Windows home and two others made from parts (a home with a space, a
/// name with letters past ASCII), so no personal path is written here.
fn homes() -> [(String, String); 3] {
    let windows = format!("C:{}Users{}Åsa", '\\', '\\');
    let linux = ["", "home", "zoë"].join("/");
    let mac = ["", "Users", "a b"].join("/");
    [
        (windows, "Åsa".into()),
        (linux, "zoë".into()),
        (mac, "a b".into()),
    ]
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
        ("reports_keep_20", keep(&root.join("kept"))),
        ("report_log_tail", log_tail(&root.join("run"))),
        ("crashed_run_forgotten_in_play", forgotten()),
        ("issue_form_filled_in", filled_in()),
    ];
    let _ = std::fs::remove_dir_all(&root);
    checks
}

fn filled_in() -> Result<String, String> {
    let [(home, user), ..] = homes();
    let me = Identity {
        home: home.clone(),
        user,
        host: "box".into(),
    };
    let note = format!("Sword shows as a pistol & more\nsaved in {home}/saves");
    let url = issue_url(&note, "0.1.0 (abc)", "kjk-report-1.zip", &me);
    let wanted = [
        "template=bug.yml",
        "&title=Sword%20shows%20as%20a%20pistol%20%26%20more&",
        "&version=0.1.0%20%28abc%29&",
        "&report=kjk-report-1.zip",
    ];
    let long = issue_url(&"é".repeat(9000), "v", "r", &me);
    check(
        wanted.iter().all(|part| url.contains(part))
            && url.contains("&happened=")
            && !url.contains("%C3%85sa")
            && long.len() < 8000
            && issue_url("", "v", "r", &me).contains("&version=v&report=r")
            && !issue_url("", "v", "r", &me).contains("title="),
        format!("{url} ({} bytes for a long note)", long.len()),
    )
}

fn scrubs() -> Result<String, String> {
    let wrong: Vec<String> = homes()
        .iter()
        .flat_map(|(home, user)| {
            let lower = home.to_lowercase().replace('\\', "/");
            [
                (
                    format!("{home}{}kjk{}saves", SEP[0], SEP[0]),
                    format!("~{}kjk{}saves", SEP[0], SEP[0]),
                ),
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
    let mut archive =
        ::zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes).map_err(|error| error.to_string())?;
        if bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
        {
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
        part(
            "kjk.log",
            &format!("run folder: {home}/.config/kjk/logs on box-7 for {user}"),
        ),
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
    check(
        !kept.contains("host.id") && kept.contains("sound.music"),
        kept,
    )
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
    let (zipped, dropped) = zip(
        vec![part("note.txt", "hi"), flight(1), flight(2), flight(3)],
        &me,
        25 * 1024 * 1024,
    );
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

/// A run's log over `LOG_KEPT` goes in as its last whole lines.
fn log_tail(run: &Path) -> Result<String, String> {
    std::fs::create_dir_all(run).map_err(|error| error.to_string())?;
    let log: String = (0..200_000)
        .map(|n| format!("log line {n:06} of a long run\n"))
        .collect();
    std::fs::write(run.join("kjk.log"), &log).map_err(|error| error.to_string())?;
    let parts = gather(run, "", None, None);
    let text = parts
        .iter()
        .find(|part| part.name == "kjk.log")
        .map(|part| String::from_utf8_lossy(&part.bytes).into_owned())
        .unwrap_or_else(String::new);
    check(
        log.len() > LOG_KEPT
            && text.len() <= LOG_KEPT
            && text.starts_with("log line ")
            && text.ends_with("log line 199999 of a long run\n"),
        format!("{} of {} bytes kept", text.len(), log.len()),
    )
}

/// A crashed run is reported from the main menu, not once a game is on.
fn forgotten() -> Result<String, String> {
    let mut world = World::new();
    world.insert_resource(CrashedRun(PathBuf::from("crashed")));
    game_started(&mut world);
    check(
        !world.contains_resource::<CrashedRun>(),
        "a game's start forgets the crashed run".into(),
    )
}
