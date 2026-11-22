//! Writes every test-kit fixture into the directory given as the only argument.

fn main() {
    let dir = std::env::args().nth(1).expect("usage: write_fixtures <dir>");
    std::fs::create_dir_all(&dir).expect("creating the directory");
    for (name, bytes) in speedyspec_wp_media_testkit::all() {
        std::fs::write(std::path::Path::new(&dir).join(name), bytes).expect("writing a fixture");
    }
}
