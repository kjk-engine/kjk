//! Reads what the git index would commit and refuses shapes that must never
//! ship: working memory, machine-local files, game data, retail addresses,
//! decompiler names, tests outside `crates/approved_tests`, personal paths
//! and addresses, and the walkthrough copied from IGN.
//!
//! It also refuses the shapes of code that came from a GPL reference
//! (OpenJK's and Quake's identifiers, "port of" next to a reference's name):
//! the parts that once had them were written afresh from behaviour specs.
//!
//! It is a grep over fixed shapes. Passing it proves only that those shapes
//! are absent; it says nothing about where code came from.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

use crate::{git, repo_root};

/// Exempt from the content scans only; the path rules have no exemptions.
const CONTENT_EXEMPT: &[&str] = &[
    // Cargo writes build metadata and hashes into it.
    "Cargo.lock",
    // This check has to spell the shapes it hunts for.
    "xtask/src/publish_check.rs",
];

const APPROVED_TESTS: &str = "crates/approved_tests/";

/// Files from a KOTOR, TSL or Jedi Academy install, by extension.
const GAME_DATA: &[&str] = &[
    "bif", "key", "erf", "rim", "mod", "sav", "tlk", "2da", "gff", "mdl", "mdx", "tpc", "wok",
    "pwk", "dwk", "lyt", "vis", "ncs", "dlg", "utc", "utd", "utp", "utt", "utw", "ute", "uts",
    "utm", "uti", "git", "are", "ifo", "jrl", "ssf", "lip", "bik", "pk3", "glm", "gla", "bsp",
];

const KEY_MATERIAL: &[&str] = &["pem", "p12", "pfx", "jks", "keystore", "ppk"];

const PREBUILT: &[&str] = &["exe", "dll", "so", "dylib", "pdb", "msi"];

/// The 32-bit retail images (KOTOR, TSL, JA) load at 0x0040_0000 and none of
/// them reaches 0x00c0_0000. A hex literal in that range reads as an address.
const IMAGE_START: u64 = 0x0040_0000;
const IMAGE_END: u64 = 0x00c0_0000;

/// Placeholder prefixes that Ghidra and IDA give unnamed symbols, followed by
/// `_` and a hex address.
const DECOMPILER_PREFIXES: &[&str] = &[
    "FUN", "DAT", "LAB", "PTR", "sub", "loc", "off", "unk", "byte", "word", "dword", "qword",
];

/// Placeholder names after `/Users/`, `/home/` or `C:\\Users\\` that name
/// nobody; a name starting with `<`, `$`, `%` or `{` is one too.
const PLACEHOLDER_HOMES: &[&str] = &[
    "someone", "you", "me", "user", "username", "runner", "Shared",
];

/// Hosts of IGN's guide, whose text must never be copied in.
const IGN_HOSTS: &[&str] = &["ign.com/wikis", "oyster.ignimgs.com"];

/// BLAKE3 of the owner's and contributors' email addresses, lowercased
/// (`git log --format='%ae%n%ce' | sort -u | cargo xtask email-hash`, bots
/// left out). Hashes, so the check does not itself publish them.
const PERSONAL_EMAILS: &[&str] = &[
    "1dea81d905474d6cc98dfde4f8adbef7ff9df3f8c093f66bc151f0c61ef62e75",
    "220fab8e5459dcbf34a36dded16d89d0790f1f1fdc2887513b4938ef6a23ba08",
    "6a32dfc96d4c6de8dfce23e6ee335fa474dc68f41b7242dae6d0cdc20d862e8d",
    "a0bcc0a4cdb3bdd5c40031b1c857d90b137572dbf73a4c71af89526f112fc547",
    "e052600b73ceb1707aa7c83b89d45e70bf0fe42504c585e4ee75eda60a7df171",
];

/// BLAKE3 of the owner's names and handles, lowercased, each matched as a
/// whole word (`cargo xtask email-hash` hashes them too): the public
/// repository names no one.
const PERSONAL_NAMES: &[&str] = &[
    "ce54228472d542751d39bbaf8447ad7e1011839190bcc85a38d42bdd93c3d14f",
    "f22ac3b85c4ab71baeefb1aac0c1677aa25e91a2a1c939a60904f2a06d340eee",
    "dc51c1428dfee92bd738deec71cb4f5c36ca0cb9e98678c8e248355a534a4a77",
    "30432a4877d787da994e954e973d8b38637e3d33d535b9dd17dee35276843398",
    "d61287c84a223c24602f23c74f0392619aba9a1db46bdac1f54a775a8116d2e2",
];

/// Prefixes of OpenJK's and Quake III's identifiers (`PM_Weapon`,
/// `WP_SaberCanBlock`, `NPC_Jedi_Pain`, …), followed by a letter or digit.
const REFERENCE_PREFIXES: &[&str] = &["PM", "WP", "BG", "CG", "G2", "RB", "NPC"];

/// Other identifiers of those codebases, whole or as a name's start.
const REFERENCE_NAMES: &[&str] = &["saberMoveData", "pmove_t", "Q_rand", "Q_irand"];

/// Saying a piece of code came from a reference, when one is named beside it.
const DERIVATION_PHRASES: &[&str] = &["port of", "ported from", "translated from", "adapted from"];
const REFERENCES: &[&str] = &["openjk", "quake", "reone", "xoreos"];

/// The owner-approved places that may name the references and say how they
/// were used: the licence notice and the contributors' rules.
const PROVENANCE_FILES: &[&str] = &["NOTICE", "README.md", "CONTRIBUTING.md", "AGENT.md"];

/// Names with a reference prefix that are not the references': the games'
/// own NWScript constants (`NPC_PLAYER`, TSL's `NPC_ATTON`, `NPC_AISTYLE_*`),
/// their modules' waypoint tags (Tatooine's `WP_tat18_bantha1_03`) and ours.
/// A name ending in `_` allows every name it starts.
const PROVENANCE_NAMES: &[&str] = &[
    "NPC_PLAYER",
    "NPC_ATTON",
    "NPC_BAO_DUR",
    "NPC_KREIA",
    "NPC_AISTYLE_",
    "NPC_TAGS",
    "NPC_CATALOG",
    "NPC_STEPS",
    "WP_tat18_",
];

struct Finding {
    path: String,
    line: usize,
    what: String,
}

pub fn run(args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("usage: cargo xtask publish-check".to_string());
    }
    let files = index_files()?;
    let mut findings = Vec::new();
    for (path, content) in &files {
        if let Some(what) = path_leak(path) {
            findings.push(Finding {
                path: path.clone(),
                line: 0,
                what: what.to_string(),
            });
            continue;
        }
        if CONTENT_EXEMPT.contains(&path.as_str()) {
            continue;
        }
        let text = String::from_utf8_lossy(content);
        scan_text(path, &text, &mut findings);
        // A route names the games' own objects (`WP_GASDEATH5`), never code.
        if !PROVENANCE_FILES.contains(&path.as_str()) && !path.ends_with(".route") {
            scan_provenance(path, &text, &mut findings);
        }
    }

    if findings.is_empty() {
        println!(
            "publish-check: {} files in the index, no findings",
            files.len()
        );
        return Ok(());
    }
    for finding in &findings {
        eprintln!("{}:{}: {}", finding.path, finding.line, finding.what);
    }
    Err(format!(
        "publish-check: {} findings in {} files. `git restore --staged <path>` unstages a file \
         without touching it on disk.",
        findings.len(),
        files.len()
    ))
}

/// `cargo xtask email-hash`: each line of stdin, an address, as the hash
/// `PERSONAL_EMAILS` keeps.
pub fn email_hashes() -> Result<(), String> {
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|error| format!("stdin: {error}"))?;
        let address = line.trim();
        if !address.is_empty() {
            println!("{}", email_hash(address));
        }
    }
    Ok(())
}

fn email_hash(address: &str) -> String {
    blake3::hash(address.to_ascii_lowercase().as_bytes())
        .to_hex()
        .to_string()
}

/// Every path in the index with its staged content.
fn index_files() -> Result<Vec<(String, Vec<u8>)>, String> {
    let listing = git(&["ls-files", "--stage", "-z"])?;
    let mut entries = Vec::new();
    for record in listing.split(|&byte| byte == 0).filter(|r| !r.is_empty()) {
        let record = String::from_utf8_lossy(record);
        let (meta, path) = record
            .split_once('\t')
            .ok_or_else(|| format!("unexpected ls-files record: {record}"))?;
        let object = meta
            .split(' ')
            .nth(1)
            .ok_or_else(|| format!("unexpected ls-files record: {record}"))?;
        entries.push((path.to_string(), object.to_string()));
    }

    let mut child = Command::new("git")
        .current_dir(repo_root())
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git cat-file: {error}"))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let objects: String = entries
        .iter()
        .map(|(_, object)| format!("{object}\n"))
        .collect();
    let writer = std::thread::spawn(move || stdin.write_all(objects.as_bytes()));

    let mut reader = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut files = Vec::with_capacity(entries.len());
    for (path, object) in entries {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|error| format!("git cat-file: {error}"))?;
        let size: usize = header
            .split_whitespace()
            .nth(2)
            .and_then(|size| size.parse().ok())
            .ok_or_else(|| format!("git cat-file {object}: unexpected header {header:?}"))?;
        let mut content = vec![0; size + 1];
        reader
            .read_exact(&mut content)
            .map_err(|error| format!("git cat-file {object}: {error}"))?;
        content.pop();
        files.push((path, content));
    }
    writer
        .join()
        .expect("writer thread does not panic")
        .map_err(|error| format!("git cat-file: {error}"))?;
    child
        .wait()
        .map_err(|error| format!("git cat-file: {error}"))?;
    Ok(files)
}

fn path_leak(path: &str) -> Option<&'static str> {
    if path == "context" || path.starts_with("context/") {
        return Some("working memory under context/");
    }
    let mut dirs = path.split('/').rev().skip(1);
    if dirs.clone().any(|dir| dir == "WALKTHROUGHS") {
        return Some("IGN's walkthrough under WALKTHROUGHS/");
    }
    if dirs.any(|dir| dir == "journey") {
        return Some("the journey timeline under journey/");
    }
    if path.starts_with("design/promo/") {
        return Some("promotion material under design/promo/");
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if name == ".DS_Store" {
        return Some("Finder's .DS_Store");
    }
    if name == ".env" || (name.starts_with(".env.") && name != ".env.example") {
        return Some("machine-local .env");
    }
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    let extension = extension.as_deref().unwrap_or("");
    if GAME_DATA.contains(&extension) {
        return Some("game data file type");
    }
    if KEY_MATERIAL.contains(&extension) {
        return Some("key or certificate material");
    }
    if PREBUILT.contains(&extension) {
        return Some("executable or library image");
    }
    if !path.starts_with(APPROVED_TESTS) && path.split('/').rev().skip(1).any(|dir| dir == "tests")
    {
        return Some("tests/ directory outside crates/approved_tests");
    }
    None
}

fn scan_text(path: &str, text: &str, findings: &mut Vec<Finding>) {
    let rust_outside_approved = path.ends_with(".rs") && !path.starts_with(APPROVED_TESTS);
    for (index, line) in text.lines().enumerate() {
        let mut report = |what: String| {
            findings.push(Finding {
                path: path.to_string(),
                line: index + 1,
                what,
            });
        };
        if let Some(literal) = address_literal(line) {
            report(format!("retail-address-shaped literal {literal}"));
        }
        if let Some(name) = decompiler_name(line) {
            report(format!("decompiler placeholder name {name}"));
        }
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {
            report("pasted private key".to_string());
        }
        if rust_outside_approved && is_test_marker(line.trim_start()) {
            report("test outside crates/approved_tests".to_string());
        }
        if let Some(home) = home_path(line) {
            report(format!("personal path {home}"));
        }
        if let Some(host) = IGN_HOSTS.iter().find(|host| line.contains(*host)) {
            report(format!("link into IGN's guide ({host})"));
        }
        if emails(line).any(|address| PERSONAL_EMAILS.contains(&email_hash(address).as_str())) {
            report("a personal email address".to_string());
        }
        if line
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| !word.is_empty() && PERSONAL_NAMES.contains(&email_hash(word).as_str()))
        {
            report("a personal name".to_string());
        }
    }
}

/// The first shape of a reference's code on each line.
fn scan_provenance(path: &str, text: &str, origins: &mut Vec<Finding>) {
    for (index, line) in text.lines().enumerate() {
        if let Some(what) = reference_shape(line) {
            origins.push(Finding {
                path: path.to_string(),
                line: index + 1,
                what,
            });
        }
    }
}

fn reference_shape(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if !is_ident(bytes[at]) {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && is_ident(bytes[at]) {
            at += 1;
        }
        let name = &line[start..at];
        if is_reference_name(name) {
            return Some(format!("reference identifier {name}"));
        }
    }
    let lower = line.to_ascii_lowercase();
    let phrase = DERIVATION_PHRASES.iter().find(|p| lower.contains(*p))?;
    let reference = REFERENCES.iter().find(|r| lower.contains(*r))?;
    Some(format!("\"{phrase}\" beside {reference}"))
}

fn is_reference_name(name: &str) -> bool {
    let allowed = PROVENANCE_NAMES.iter().any(|allowed| {
        if allowed.ends_with('_') {
            name.starts_with(allowed)
        } else {
            name == *allowed
        }
    });
    if allowed {
        return false;
    }
    let prefixed = REFERENCE_PREFIXES.iter().any(|prefix| {
        name.strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('_'))
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_alphanumeric()))
    });
    prefixed
        || REFERENCE_NAMES
            .iter()
            .any(|reference| name.starts_with(reference))
}

/// A home folder with a real name in it: `/Users/<name>`, `/home/<name>`,
/// `C:\Users\<name>`.
fn home_path(line: &str) -> Option<String> {
    for root in ["/Users/", "/home/", "C:\\Users\\", "C:/Users/"] {
        let mut at = 0;
        while let Some(offset) = line[at..].find(root) {
            let start = at + offset;
            at = start + root.len();
            let name: String = line[at..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                .collect();
            let placeholder = name.is_empty()
                || PLACEHOLDER_HOMES.contains(&name.as_str())
                || line[at..].starts_with(['<', '$', '%', '{']);
            if !placeholder {
                return Some(format!("{root}{name}"));
            }
        }
    }
    None
}

/// The email-shaped words of a line.
fn emails(line: &str) -> impl Iterator<Item = &str> {
    let bytes = line.as_bytes();
    let local = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-');
    let domain = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-');
    line.match_indices('@').filter_map(move |(at, _)| {
        let start = (0..at).rev().take_while(|&i| local(bytes[i])).last()?;
        let end = (at + 1..bytes.len())
            .take_while(|&i| domain(bytes[i]))
            .last()?
            + 1;
        let address = line[start..end].trim_end_matches('.');
        address[at - start + 1..].contains('.').then_some(address)
    })
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The hex digits (and `_` separators) starting at `from`, and where they end.
fn hex_run(bytes: &[u8], from: usize) -> (String, usize) {
    let mut end = from;
    while end < bytes.len() && (bytes[end].is_ascii_hexdigit() || bytes[end] == b'_') {
        end += 1;
    }
    let digits = String::from_utf8_lossy(&bytes[from..end]).replace('_', "");
    (digits, end)
}

fn address_literal(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut at = 0;
    while let Some(offset) = line[at..].find(['x', 'X']) {
        let x = at + offset;
        at = x + 1;
        if x == 0 || bytes[x - 1] != b'0' || (x >= 2 && is_ident(bytes[x - 2])) {
            continue;
        }
        let (digits, end) = hex_run(bytes, x + 1);
        if digits.is_empty() || digits.len() > 16 || bytes.get(end).is_some_and(|&b| is_ident(b)) {
            continue;
        }
        let value = u64::from_str_radix(&digits, 16).ok()?;
        if (IMAGE_START..IMAGE_END).contains(&value) {
            return Some(line[x - 1..end].to_string());
        }
    }
    None
}

fn decompiler_name(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    for prefix in DECOMPILER_PREFIXES {
        let pattern = format!("{prefix}_");
        let mut at = 0;
        while let Some(offset) = line[at..].find(&pattern) {
            let start = at + offset;
            at = start + pattern.len();
            if start > 0 && is_ident(bytes[start - 1]) {
                continue;
            }
            let (digits, end) = hex_run(bytes, at);
            let bounded = !bytes.get(end).is_some_and(|&b| is_ident(b));
            if (6..=16).contains(&digits.len()) && bounded {
                return Some(line[start..end].to_string());
            }
        }
    }
    None
}

fn is_test_marker(line: &str) -> bool {
    line.starts_with("#[test]")
        || line.starts_with("#[cfg(test)]")
        || line.starts_with("#[tokio::test")
        || line
            .strip_prefix("mod tests")
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '{', ';']))
}
