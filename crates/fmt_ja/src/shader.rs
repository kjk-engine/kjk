//! `.shader` scripts: each top-level block is one shader, named by its path;
//! lines directly inside are shader directives (`cull`, `surfaceparm`, …),
//! and nested blocks are stages (`map`, `blendFunc`, `rgbGen`, `tcMod`, …).
//! Keywords are matched without case, as the engine does.

use crate::script::{self, Block, ScriptError};

#[derive(Debug, Clone, PartialEq)]
pub struct Shader {
    pub name: String,
    pub directives: Vec<Vec<String>>,
    pub stages: Vec<Vec<Vec<String>>>,
}

/// A shader block that does not fit the shape: a stage inside a stage, or
/// no name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderRefusal {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShaderFile {
    pub shaders: Vec<Shader>,
    pub refused: Vec<ShaderRefusal>,
    /// Words outside every shader block.
    pub stray: Vec<Vec<String>>,
}

pub fn parse(bytes: &[u8]) -> Result<ShaderFile, ScriptError> {
    let root = script::parse(bytes)?;
    let mut file = ShaderFile {
        stray: root.lines,
        ..ShaderFile::default()
    };
    for block in root.children {
        let name = block.name.join(" ");
        if block.name.len() != 1 {
            file.refused.push(ShaderRefusal {
                name,
                reason: format!("name is {} words", block.name.len()),
            });
            continue;
        }
        if let Some(nested) = block
            .children
            .iter()
            .find(|stage| !stage.children.is_empty())
        {
            file.refused.push(ShaderRefusal {
                name,
                reason: format!("a stage holds a block ({:?})", nested.children[0].name),
            });
            continue;
        }
        let Block {
            lines, children, ..
        } = block;
        file.shaders.push(Shader {
            name: name.to_ascii_lowercase(),
            directives: lines,
            stages: children.into_iter().map(|stage| stage.lines).collect(),
        });
    }
    Ok(file)
}
