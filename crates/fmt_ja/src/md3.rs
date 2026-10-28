//! MD3 meshes (`IDP3`, version 15), the Quake 3 vertex-animated format JA
//! still uses for some effect models. Only the first frame is read.
//!
//! Header (108 bytes): ident, version, name (64), flags, frame, tag,
//! surface and skin counts, frame, tag and surface offsets, end. Each
//! surface (from its own start): ident, name (64), flags, frame, shader,
//! vertex and triangle counts, then the offsets of its triangles (three
//! vertex indices each), shaders (a 64-byte name and an index), texture
//! coordinates (two floats a vertex) and vertices (per frame, per vertex:
//! three `i16` in 1/64 units and a packed normal), and its end.

use crate::ghoul2::Ghoul2Error;

const HEADER: usize = 108;
const NAME: usize = 64;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Md3 {
    pub name: String,
    pub surfaces: Vec<Surface>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Surface {
    pub name: String,
    pub shaders: Vec<String>,
    /// The first frame's vertices, in model units.
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub triangles: Vec<[u32; 3]>,
}

fn slice<'a>(bytes: &'a [u8], at: usize, len: usize, what: &str) -> Result<&'a [u8], Ghoul2Error> {
    at.checked_add(len)
        .and_then(|end| bytes.get(at..end))
        .ok_or_else(|| Ghoul2Error::Truncated {
            what: what.to_string(),
        })
}

fn i32_at(bytes: &[u8], at: usize, what: &str) -> Result<i32, Ghoul2Error> {
    Ok(i32::from_le_bytes(
        slice(bytes, at, 4, what)?.try_into().expect("4 bytes"),
    ))
}

fn count_at(bytes: &[u8], at: usize, what: &str) -> Result<usize, Ghoul2Error> {
    usize::try_from(i32_at(bytes, at, what)?).map_err(|_| Ghoul2Error::Bad(format!("{what} < 0")))
}

fn name_at(bytes: &[u8], at: usize, what: &str) -> Result<String, Ghoul2Error> {
    let raw = slice(bytes, at, NAME, what)?;
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    Ok(String::from_utf8_lossy(&raw[..end]).into_owned())
}

pub fn parse(bytes: &[u8]) -> Result<Md3, Ghoul2Error> {
    if slice(bytes, 0, 4, "md3 ident")? != b"IDP3" {
        return Err(Ghoul2Error::Signature("md3"));
    }
    let version = i32_at(bytes, 4, "md3 version")?;
    if version != 15 {
        return Err(Ghoul2Error::Bad(format!(
            "md3 version {version}, expected 15"
        )));
    }
    let name = name_at(bytes, 8, "md3 name")?;
    let surfaces = count_at(bytes, 84, "md3 surface count")?;
    let mut at = count_at(bytes, 100, "md3 surface offset")?;
    if HEADER > bytes.len() {
        return Err(Ghoul2Error::Truncated {
            what: "md3 header".into(),
        });
    }
    let mut out = Md3 {
        name,
        surfaces: Vec::with_capacity(surfaces.min(64)),
    };
    for _ in 0..surfaces {
        let surface = slice(bytes, at, HEADER, "md3 surface")?;
        let name = name_at(surface, 4, "md3 surface name")?;
        let shaders = count_at(surface, 76, "md3 shader count")?;
        let vertices = count_at(surface, 80, "md3 vertex count")?;
        let triangles = count_at(surface, 84, "md3 triangle count")?;
        let offset = |field: usize, what: &str| -> Result<usize, Ghoul2Error> {
            Ok(at + count_at(surface, field, what)?)
        };
        let (ofs_triangles, ofs_shaders, ofs_st, ofs_xyz, ofs_end) = (
            offset(88, "md3 triangle offset")?,
            offset(92, "md3 shader offset")?,
            offset(96, "md3 st offset")?,
            offset(100, "md3 vertex offset")?,
            offset(104, "md3 surface end")?,
        );
        let mut parsed = Surface {
            name,
            ..Surface::default()
        };
        for s in 0..shaders {
            parsed
                .shaders
                .push(name_at(bytes, ofs_shaders + s * (NAME + 4), "md3 shader")?);
        }
        for t in 0..triangles {
            let base = ofs_triangles + t * 12;
            let mut triangle = [0u32; 3];
            for (k, index) in triangle.iter_mut().enumerate() {
                let vertex = count_at(bytes, base + k * 4, "md3 triangle")?;
                if vertex >= vertices {
                    return Err(Ghoul2Error::Bad(format!(
                        "md3 surface {} triangle {t} indexes past {vertices} vertices",
                        parsed.name
                    )));
                }
                *index = vertex as u32;
            }
            parsed.triangles.push(triangle);
        }
        for v in 0..vertices {
            let st = slice(bytes, ofs_st + v * 8, 8, "md3 st")?;
            let f = |i: usize| f32::from_le_bytes(st[i..i + 4].try_into().expect("4 bytes"));
            parsed.uvs.push([f(0), f(4)]);
            let xyz = slice(bytes, ofs_xyz + v * 8, 6, "md3 vertex")?;
            let c = |i: usize| f32::from(i16::from_le_bytes([xyz[i], xyz[i + 1]])) / 64.0;
            parsed.positions.push([c(0), c(2), c(4)]);
        }
        out.surfaces.push(parsed);
        at = ofs_end;
    }
    Ok(out)
}
