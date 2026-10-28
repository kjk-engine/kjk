mod stamp;

/// Stamps the commit into the game as `KJK_GIT_SHA` (`stamp.rs`), and the
/// code that reads JA's shaders as `KJK_JA_SHADERS`.
fn main() {
    // What the game is built from: every crate but the two built on it
    // (`launcher`, `approved_tests`), so a change to a verb or a test does
    // not rebuild the game. A tree dirty only there is stamped `-dirty`
    // here once the game next builds; the binary's own stamp
    // (`launcher/build.rs`) watches every crate.
    let sources: Vec<std::path::PathBuf> = std::fs::read_dir("..")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some("launcher" | "approved_tests")
            )
        })
        .map(|entry| entry.path())
        .collect();
    stamp::git_sha(&sources);
    // A stamp of the code that reads JA's shaders into the shader-to-texture
    // map kept in the store (`viewer_ja::shader_textures`): FNV-1a over the
    // files' bytes.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for path in ["src/viewer_ja.rs", "../fmt_ja/src/shader.rs"] {
        println!("cargo:rerun-if-changed={path}");
        for byte in std::fs::read(path).unwrap_or_else(|_| Vec::new()) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=KJK_JA_SHADERS={hash:016x}");
}
