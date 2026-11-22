//! Writes the benchmark's three files (bench.png, bench.mp3, bench.wav) into the directory given.

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: write_bench_fixtures <dir>"));
    std::fs::create_dir_all(&dir).expect("creating the directory");
    let kit = |name: &str, bytes: Vec<u8>| std::fs::write(dir.join(name), bytes).expect("writing a fixture");
    kit("bench.png", speedyspec_wp_media_testkit::png(640, 480));
    kit("bench.mp3", speedyspec_wp_media_testkit::mp3_with_id3v2(2000, "T", "A", Some(&speedyspec_wp_media_testkit::png(300, 300))));
    kit("bench.wav", speedyspec_wp_media_testkit::wav_pcm16(30.0, 44100, 2));
}
