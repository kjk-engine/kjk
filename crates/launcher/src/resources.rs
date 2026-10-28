use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fmt_odyssey::ResType;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game, Install};

fn open(
    discovery: &Discovery,
    game: &str,
    module: Option<&str>,
) -> Result<(Install, Resolver), String> {
    let game = Game::from_key(game).ok_or(format!("unknown game {game:?}: use k1 or tsl"))?;
    let install = discovery
        .get(game)
        .ok_or(format!(
            "{}: not found (set {})",
            game.key(),
            game.env_var()
        ))?
        .clone();
    let index = DirIndex::build(&install.data_root).map_err(|error| error.to_string())?;
    let resolver = Resolver::open(game, &index, module, TextureQuality::High)
        .map_err(|error| error.to_string())?;
    Ok((install, resolver))
}

fn ja_stack(discovery: &Discovery) -> Result<(Install, vfs::ja::JaStack), String> {
    let install = discovery
        .get(Game::JediAcademy)
        .ok_or("ja: not found (set KJK_JA)")?
        .clone();
    let index = DirIndex::build(&install.data_root).map_err(|error| error.to_string())?;
    Ok((install, vfs::ja::JaStack::open(&index)))
}

fn ls_ja(discovery: &Discovery, pattern: &str, only_shadowed: bool) -> ExitCode {
    let (_, stack) = match ja_stack(discovery) {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut listed = 0;
    for (name, resolved) in stack.all() {
        if !matches(pattern, name) || (only_shadowed && resolved.shadowed.is_empty()) {
            continue;
        }
        listed += 1;
        let source = stack.source_name(resolved.source);
        if resolved.shadowed.is_empty() {
            outln!("{name}	{}	{source}", resolved.size);
        } else {
            let shadowed: Vec<String> = resolved
                .shadowed
                .iter()
                .map(|lower| {
                    let name_of = stack.source_name(*lower);
                    if !only_shadowed {
                        return name_of;
                    }
                    let same =
                        stack.read(name, resolved.source).ok() == stack.read(name, *lower).ok();
                    format!("{name_of} ({})", if same { "same" } else { "differs" })
                })
                .collect();
            outln!(
                "{name}	{}	{source}	shadows {}",
                resolved.size,
                shadowed.join(", ")
            );
        }
    }
    eprintln!(
        "{listed} names from {} pk3 and {} loose files",
        stack.pk3s.len(),
        stack.loose.len()
    );
    ExitCode::SUCCESS
}

pub fn ls(
    discovery: &Discovery,
    game: &str,
    pattern: Option<&str>,
    module: Option<&str>,
    only_shadowed: bool,
) -> ExitCode {
    if Game::from_key(game) == Some(Game::JediAcademy) {
        let pattern = pattern.unwrap_or("*").to_ascii_lowercase();
        return ls_ja(discovery, &pattern, only_shadowed);
    }
    let (_, resolver) = match open(discovery, game, module) {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let pattern = pattern.unwrap_or("*").to_ascii_lowercase();
    let mut listed = 0;
    let mut by_layer: BTreeMap<&str, usize> = BTreeMap::new();
    let mut verdicts: BTreeMap<&str, usize> = BTreeMap::new();
    for ((resref, restype), resolved) in resolver.all() {
        let name = format!("{resref}.{restype}");
        if !matches(&pattern, &name) || (only_shadowed && resolved.shadowed.is_empty()) {
            continue;
        }
        listed += 1;
        *by_layer.entry(&resolved.layer.name).or_insert(0) += 1;
        let shadowed: Vec<String> = resolved
            .shadowed
            .iter()
            .map(|(layer, location)| {
                if !only_shadowed {
                    return layer.name.clone();
                }
                let same = resolved.location.read().ok() == location.read().ok();
                let verdict = if same { "same" } else { "differs" };
                *verdicts.entry(verdict).or_insert(0) += 1;
                format!("{} ({verdict})", layer.name)
            })
            .collect();
        if shadowed.is_empty() {
            outln!(
                "{name}\t{}\t{}",
                resolved.location.size,
                resolved.layer.name
            );
        } else {
            outln!(
                "{name}\t{}\t{}\tshadows {}",
                resolved.location.size,
                resolved.layer.name,
                shadowed.join(", ")
            );
        }
    }
    if !only_shadowed {
        for layer in &resolver.streams {
            for ((resref, restype), location) in &layer.resources {
                let name = format!("{resref}.{restype}");
                if matches(&pattern, &name) {
                    listed += 1;
                    outln!("{name}	{}	stream {}", location.size, layer.name);
                }
            }
        }
    }
    eprintln!(
        "{listed} resources from {} layers and {} stream folders",
        resolver.layers.len(),
        resolver.streams.len()
    );
    for layer in &resolver.streams {
        eprintln!("  stream {}: {} names", layer.name, layer.resources.len());
    }
    for layer in &resolver.layers {
        eprintln!(
            "  layer {}: {} names, {} listed win here",
            layer.name,
            layer.resources.len(),
            by_layer.get(layer.name.as_str()).copied().unwrap_or(0)
        );
    }
    if only_shadowed {
        eprintln!("  shadowed copies by content: {verdicts:?}");
    }
    eprintln!("  layer problems: {}", resolver.problems.len());
    for problem in resolver.problems.iter().take(10) {
        eprintln!("    {}: {}", problem.source, problem.problem);
    }
    ExitCode::SUCCESS
}

fn extract_ja(discovery: &Discovery, name: &str, out: Option<PathBuf>) -> ExitCode {
    let (install, stack) = match ja_stack(discovery) {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let name = fmt_ja::game_path(name);
    let all = stack.all();
    let Some(resolved) = all.get(name.as_str()) else {
        eprintln!("{name}: not found");
        return ExitCode::FAILURE;
    };
    let out = out.unwrap_or_else(|| crate::runtime::artifacts().join("extract").join("ja"));
    if inside(&out, &install.root) {
        eprintln!("refusing to write into the game install: {}", out.display());
        return ExitCode::FAILURE;
    }
    let bytes = match stack.read(&name, resolved.source) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let target = out.join(&name);
    let written = target
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&target, &bytes));
    if let Err(error) = written {
        eprintln!("{}: {error}", target.display());
        return ExitCode::FAILURE;
    }
    outln!(
        "{}	{} bytes	from {}",
        target.display(),
        bytes.len(),
        stack.source_name(resolved.source)
    );
    ExitCode::SUCCESS
}

pub fn extract(
    discovery: &Discovery,
    game: &str,
    resource: &str,
    module: Option<&str>,
    out: Option<PathBuf>,
) -> ExitCode {
    if Game::from_key(game) == Some(Game::JediAcademy) {
        return extract_ja(discovery, resource, out);
    }
    let (install, resolver) = match open(discovery, game, module) {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let Some((resref, restype)) = resource
        .rsplit_once('.')
        .and_then(|(resref, extension)| Some((resref, ResType::from_extension(extension)?)))
    else {
        eprintln!("{resource:?}: expected resref.ext with a known extension");
        return ExitCode::from(2);
    };
    let Some(resolved) = resolver.find(resref, restype) else {
        eprintln!("{resource}: not found");
        return ExitCode::FAILURE;
    };
    let out = out.unwrap_or_else(|| {
        crate::runtime::artifacts()
            .join("extract")
            .join(install.game.key())
    });
    if inside(&out, &install.root) {
        eprintln!("refusing to write into the game install: {}", out.display());
        return ExitCode::FAILURE;
    }
    let bytes = match resolved.location.read() {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{resource}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let target = out.join(format!("{}.{restype}", resref.to_ascii_lowercase()));
    let written = std::fs::create_dir_all(&out).and_then(|()| std::fs::write(&target, &bytes));
    if let Err(error) = written {
        eprintln!("{}: {error}", target.display());
        return ExitCode::FAILURE;
    }
    outln!(
        "{}\t{} bytes\tfrom {}",
        target.display(),
        bytes.len(),
        resolved.layer.name
    );
    ExitCode::SUCCESS
}

/// Whether `path` is `root` or lies under it, compared after resolving both.
/// A path that does not exist yet is judged by its nearest existing parent.
fn inside(path: &Path, root: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let mut probe = path.to_path_buf();
    loop {
        if let Ok(resolved) = probe.canonicalize() {
            return resolved.starts_with(&root);
        }
        if !probe.pop() {
            return false;
        }
    }
}

/// `*` matches any run of characters, `?` exactly one.
fn matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((star_p, star_n)) = star {
            p = star_p + 1;
            n = star_n + 1;
            star = Some((star_p, star_n + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}
