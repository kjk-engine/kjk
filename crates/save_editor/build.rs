#[path = "../game/stamp.rs"]
mod stamp;

/// Stamps the release the editor is from (`KJK_VERSION`) and its commit
/// (`KJK_GIT_SHA`), as kjk's own binary is stamped: the editor and the kjk in
/// one package are built from one commit, so they read and write one format.
fn main() {
    stamp::git_sha(&["src", "../edit/src"]);
}
