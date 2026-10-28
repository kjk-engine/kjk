//! Writes the kitchen-sink save at this build's versions into the format
//! fixtures (`sim::fixtures::DIR`), and prints its path: what `cargo xtask
//! pin` runs when a stored format moves (`bumps/README.md`).

fn main() {
    let root = approved_tests::workspace();
    let root = root.canonicalize().unwrap_or(root);
    let dir = root.join(sim::fixtures::DIR);
    std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    let path = dir.join(sim::fixtures::file_name(sim::fixtures::current()));
    let (_, save) = sim::fixtures::kitchen_sink();
    std::fs::write(&path, save.encode())
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    println!("{}", path.display());
}
