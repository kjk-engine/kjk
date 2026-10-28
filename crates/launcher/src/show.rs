//! `kjk show <game> <resref.ext>`: a resolved resource as text. GFFs print
//! as an indented tree with localized strings looked up in `dialog.tlk`;
//! 2DAs print as tab-separated rows.

use std::process::ExitCode;

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffStruct, GffValue};
use fmt_odyssey::mdl::{Model, Node};
use fmt_odyssey::texture::Texture;
use fmt_odyssey::tlk::Tlk;
use fmt_odyssey::twoda::TwoDa;
use fmt_odyssey::txi::Txi;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn float(bits: u32) -> f32 {
    f32::from_bits(bits)
}

fn value(value: &GffValue, tlk: Option<&Tlk>) -> String {
    match value {
        GffValue::Byte(v) => format!("byte {v}"),
        GffValue::Char(v) => format!("char {v}"),
        GffValue::Word(v) => format!("word {v}"),
        GffValue::Short(v) => format!("short {v}"),
        GffValue::Dword(v) => format!("dword {v}"),
        GffValue::Int(v) => format!("int {v}"),
        GffValue::Dword64(v) => format!("dword64 {v}"),
        GffValue::Int64(v) => format!("int64 {v}"),
        GffValue::Float(bits) => format!("float {}", float(*bits)),
        GffValue::Double(bits) => format!("double {}", f64::from_bits(*bits)),
        GffValue::String(bytes) => format!("string {:?}", text(bytes)),
        GffValue::ResRef(bytes) => format!("resref {:?}", text(bytes)),
        GffValue::LocString(loc) => {
            let talk = tlk
                .filter(|_| loc.strref != u32::MAX)
                .and_then(|table| table.text(loc.strref))
                .map(|bytes| format!(" = {:?}", text(bytes)));
            let own: Vec<String> = loc
                .strings
                .iter()
                .map(|(language, bytes)| format!("[{language}] {:?}", text(bytes)))
                .collect();
            format!(
                "locstring strref {}{}{}",
                loc.strref as i32,
                talk.unwrap_or_else(String::new),
                if own.is_empty() {
                    String::new()
                } else {
                    format!(" {}", own.join(" "))
                }
            )
        }
        GffValue::Void(bytes) => format!("void {} bytes", bytes.len()),
        GffValue::Orientation(bits) => format!("orientation {:?}", bits.map(float)),
        GffValue::Vector(bits) => format!("vector {:?}", bits.map(float)),
        GffValue::StrRef(strref) => format!("strref {strref}"),
        GffValue::Struct(_) | GffValue::List(_) => unreachable!("handled by tree"),
    }
}

fn model_node(node: &Node, depth: usize) {
    let pad = "  ".repeat(depth);
    let mut line = format!(
        "{pad}{} flags {:#06x} pos {:?} rot {:?}",
        node.name, node.flags, node.position, node.orientation
    );
    if !node.controllers.is_empty() {
        let kinds: Vec<String> = node
            .controllers
            .iter()
            .map(|c| {
                // A single static row: show its values too.
                if c.times.len() == 1 && !c.bezier && c.values.len() <= 4 && c.kind != 20 {
                    format!("{}={:?}", c.kind, c.values)
                } else if c.times.len() <= 6 && c.kind != 20 {
                    // A few keys (an animation's): their times and rows.
                    format!("{}@{:?}={:?}", c.kind, c.times, c.values)
                } else {
                    format!("{}x{}", c.kind, c.times.len())
                }
            })
            .collect();
        line += &format!(" controllers [{}]", kinds.join(" "));
    }
    outln!("{line}");
    if let Some(emitter) = &node.emitter {
        outln!(
            "{pad}  emitter: update {} render {} blend {} texture {:?} chunk {:?} grid {:?} spawn {} two_sided {} looped {} order {} flags {:#06x}",
            emitter.update,
            emitter.render,
            emitter.blend,
            emitter.texture,
            emitter.chunk,
            emitter.grid,
            emitter.spawn_type,
            emitter.two_sided,
            emitter.looped,
            emitter.render_order,
            emitter.flags
        );
    }
    if let Some(mesh) = &node.mesh {
        outln!(
            "{pad}  mesh: {} vertices, {} faces, textures {:?}, render {}, transparency {}, skin {}, dangly {}, aabb {}, saber {}, animateuv {:?}",
            mesh.positions.len(),
            mesh.faces.len(),
            mesh.textures,
            mesh.render,
            mesh.transparency_hint,
            mesh.skin.is_some(),
            mesh.dangly.is_some(),
            mesh.aabb.is_some(),
            mesh.saber,
            mesh.animate_uv
        );
        for i in 0..mesh.positions.len().min(3) {
            outln!(
                "{pad}    v{i} pos {:?} normal {:?} uv1 {:?} uv2 {:?}",
                mesh.positions[i],
                mesh.normals.get(i),
                mesh.uv1.get(i),
                mesh.uv2.get(i)
            );
        }
    }
    for child in &node.children {
        model_node(child, depth + 1);
    }
}

fn tree(node: &GffStruct, depth: usize, tlk: Option<&Tlk>) {
    let pad = "  ".repeat(depth);
    for field in &node.fields {
        let label = text(&field.label);
        match &field.value {
            GffValue::Struct(child) => {
                outln!("{pad}{label}: struct {}", child.id as i32);
                tree(child, depth + 1, tlk);
            }
            GffValue::List(items) => {
                outln!("{pad}{label}: list of {}", items.len());
                for (i, item) in items.iter().enumerate() {
                    outln!("{pad}  [{i}] struct {}", item.id as i32);
                    tree(item, depth + 2, tlk);
                }
            }
            other => outln!("{pad}{label}: {}", value(other, tlk)),
        }
    }
}

pub fn run(discovery: &Discovery, game: &str, resource: &str, module: Option<&str>) -> ExitCode {
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("show: use k1 or tsl");
        return ExitCode::from(2);
    };
    let Some(install) = discovery.get(game) else {
        eprintln!("{}: not found (set {})", game.key(), game.env_var());
        return ExitCode::FAILURE;
    };
    let Some((resref, restype)) = resource
        .rsplit_once('.')
        .and_then(|(resref, extension)| Some((resref, ResType::from_extension(extension)?)))
    else {
        eprintln!("{resource:?}: expected resref.ext with a known extension");
        return ExitCode::from(2);
    };
    let opened = DirIndex::build(&install.data_root)
        .map_err(|error| error.to_string())
        .and_then(|index| {
            let tlk = index
                .get("dialog.tlk")
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| Tlk::parse(&bytes).ok());
            Resolver::open(game, &index, module, TextureQuality::High)
                .map(|resolver| (resolver, tlk))
                .map_err(|error| error.to_string())
        });
    let (resolver, tlk) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(found) = resolver.find(resref, restype) else {
        eprintln!("{resource}: not found");
        return ExitCode::FAILURE;
    };
    let bytes = match found.location.read() {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{resource}: {error}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "{resource}: {} bytes from {}",
        bytes.len(),
        found.layer.name
    );
    if restype == ResType::MDL {
        let mdx_type = ResType::MDX;
        let mdx = resolver
            .find(resref, mdx_type)
            .and_then(|mdx| mdx.location.read().ok())
            .unwrap_or_else(Vec::new);
        return match Model::parse(&bytes, &mdx) {
            Ok(model) => {
                outln!(
                    "model {} class {:#04x} supermodel {:?} radius {} animations {}",
                    model.name,
                    model.classification,
                    model.supermodel,
                    model.radius,
                    model.animations.len()
                );
                for animation in &model.animations {
                    outln!(
                        "  animation {} length {} events {:?}",
                        animation.name,
                        animation.length,
                        animation.events
                    );
                    model_node(&animation.root, 2);
                }
                model_node(&model.root, 1);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    let tpc = ResType::TPC;
    let txi = ResType::TXI;
    if restype == tpc || restype == txi {
        let settings = if restype == tpc {
            match Texture::parse_tpc(&bytes) {
                Ok(texture) => {
                    outln!(
                        "texture {}x{} {:?}, {} mips, {} layers{}",
                        texture.width(),
                        texture.height(),
                        texture.format,
                        texture.layers.first().map_or(0, Vec::len),
                        texture.layers.len(),
                        if texture.cube { ", cube" } else { "" }
                    );
                    texture.txi
                }
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            Txi::parse(&bytes)
        };
        for (keyword, values) in &settings.settings {
            outln!("  {keyword} {}", values.join(" "));
        }
        for (keyword, rows) in &settings.blocks {
            outln!("  {keyword} ({} rows)", rows.len());
        }
        return ExitCode::SUCCESS;
    }
    if bytes.starts_with(b"2DA ") {
        match TwoDa::parse(&bytes) {
            Ok(table) => {
                let columns: Vec<String> = table.columns.iter().map(|c| text(c)).collect();
                outln!("\t{}", columns.join("\t"));
                for (label, row) in table.row_labels.iter().zip(&table.cells) {
                    let cells: Vec<String> = row.iter().map(|c| text(c)).collect();
                    outln!("{}\t{}", text(label), cells.join("\t"));
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        }
    } else {
        match Gff::parse(&bytes) {
            Ok(gff) => {
                outln!(
                    "{} struct {}",
                    text(&gff.file_type).trim(),
                    gff.root.id as i32
                );
                tree(&gff.root, 1, tlk.as_ref());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        }
    }
}
