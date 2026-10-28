//! `kjk texture <game> <resref>`: decodes a resolved texture (TPC first, then
//! TGA, as the games prefer) and writes its first mip as PNG, one file per
//! layer (cube faces, flip-book frames).

use std::path::PathBuf;
use std::process::ExitCode;

use fmt_odyssey::ResType;
use fmt_odyssey::texture::Texture;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

pub fn run(
    discovery: &Discovery,
    game: &str,
    resref: &str,
    module: Option<&str>,
    out: Option<PathBuf>,
) -> ExitCode {
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("texture: use k1 or tsl");
        return ExitCode::from(2);
    };
    let Some(install) = discovery.get(game) else {
        eprintln!("{}: not found (set {})", game.key(), game.env_var());
        return ExitCode::FAILURE;
    };
    let resolver = match DirIndex::build(&install.data_root)
        .map_err(|error| error.to_string())
        .and_then(|index| {
            Resolver::open(game, &index, module, TextureQuality::High).map_err(|e| e.to_string())
        }) {
        Ok(resolver) => resolver,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let tpc = ResType::TPC;
    let tga = ResType::TGA;
    let (found, is_tpc) = match resolver.find(resref, tpc) {
        Some(found) => (found, true),
        None => match resolver.find(resref, tga) {
            Some(found) => (found, false),
            None => {
                eprintln!("{resref}: no tpc or tga");
                return ExitCode::FAILURE;
            }
        },
    };
    let texture = match found
        .location
        .read()
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            if is_tpc {
                Texture::parse_tpc(&bytes)
            } else {
                Texture::parse_tga(&bytes)
            }
            .map_err(|error| error.to_string())
        }) {
        Ok(texture) => texture,
        Err(error) => {
            eprintln!("{resref}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let out = out.unwrap_or_else(|| {
        crate::runtime::artifacts()
            .join("textures")
            .join(game.key())
    });
    if let Err(error) = std::fs::create_dir_all(&out) {
        eprintln!("{}: {error}", out.display());
        return ExitCode::FAILURE;
    }
    outln!(
        "{resref}.{}: {:?}, {}x{}, {} layer(s){}, {} mips, from {}",
        if is_tpc { "tpc" } else { "tga" },
        texture.format,
        texture.width(),
        texture.height(),
        texture.layers.len(),
        if texture.cube { " (cube)" } else { "" },
        texture.layers[0].len(),
        found.layer.name
    );
    for layer in 0..texture.layers.len() {
        let Some(rgba) = texture.rgba(layer, 0) else {
            eprintln!("layer {layer}: does not decode");
            return ExitCode::FAILURE;
        };
        let name = if texture.layers.len() == 1 {
            format!("{resref}.png")
        } else {
            format!("{resref}-{layer}.png")
        };
        let path = out.join(name);
        let saved = image::save_buffer(
            &path,
            &rgba,
            texture.width() as u32,
            texture.height() as u32,
            image::ColorType::Rgba8,
        );
        if let Err(error) = saved {
            eprintln!("{}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        outln!("  {}", path.display());
    }
    ExitCode::SUCCESS
}
