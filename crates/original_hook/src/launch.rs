//! What kjk needs to know to start a game with the library in it. kjk
//! names the module and the log file in the game's environment. On macOS,
//! dyld loads the library into the game. On Windows, `kjk-inject` starts the
//! game and loads the library into it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The environment variable that names the module the library starts.
pub const MODULE_VAR: &str = "KJK_ORIGINAL_MODULE";
/// The environment variable that names the library's log file.
pub const LOG_VAR: &str = "KJK_ORIGINAL_LOG";

/// The system a game's build is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    Mac,
    Windows,
}

impl System {
    /// The target the library is built for. The Mac ports are x86_64
    /// programs, which run under Rosetta on Apple's own chips. The Windows
    /// builds are 32-bit programs. A kjk built with MinGW builds the library
    /// with MinGW too.
    pub fn target(self) -> &'static str {
        match self {
            System::Mac => "x86_64-apple-darwin",
            System::Windows if cfg!(target_env = "gnu") => "i686-pc-windows-gnu",
            System::Windows => "i686-pc-windows-msvc",
        }
    }

    fn library(self) -> &'static str {
        match self {
            System::Mac => "libkjk_original.dylib",
            System::Windows => "kjk_original.dll",
        }
    }

    fn injector(self) -> Option<&'static str> {
        match self {
            System::Mac => None,
            System::Windows => Some("kjk-inject.exe"),
        }
    }

    /// The library's files in `folder`, when all of them are there.
    pub fn files_in(self, folder: &Path) -> Option<Files> {
        let library = folder.join(self.library());
        let injector = self.injector().map(|injector| folder.join(injector));
        (library.is_file() && injector.as_ref().is_none_or(|injector| injector.is_file()))
            .then_some(Files { library, injector })
    }

    /// cargo's arguments that build the library's files, run in kjk's
    /// workspace.
    pub fn build_args(self) -> [&'static str; 8] {
        [
            "build",
            "-p",
            "original_hook",
            "--release",
            "--features",
            "game",
            "--target",
            self.target(),
        ]
    }

    /// The folder where cargo puts the files it builds, inside cargo's
    /// `target_folder`.
    pub fn built_in(self, target_folder: &Path) -> PathBuf {
        target_folder.join(self.target()).join("release")
    }
}

/// The library's files for one system.
pub struct Files {
    library: PathBuf,
    /// `kjk-inject`, on Windows.
    injector: Option<PathBuf>,
}

/// A program to run, with its arguments and the environment it adds.
pub struct Command {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl Files {
    /// The command that starts the game's program `game` with the library in
    /// it. The library will start `module` and write its log to `log`.
    pub fn start(&self, game: &Path, module: &str, log: &Path) -> Command {
        let mut env = vec![
            (MODULE_VAR.into(), module.into()),
            (LOG_VAR.into(), log.into()),
        ];
        match &self.injector {
            Some(injector) => Command {
                program: injector.clone(),
                args: vec![game.into(), self.library.clone().into()],
                env,
            },
            None => {
                env.push(("DYLD_INSERT_LIBRARIES".into(), self.library.clone().into()));
                Command {
                    program: game.to_path_buf(),
                    args: Vec::new(),
                    env,
                }
            }
        }
    }
}
