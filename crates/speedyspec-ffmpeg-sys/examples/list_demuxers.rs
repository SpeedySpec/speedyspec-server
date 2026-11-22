fn main() {
    for name in speedyspec_ffmpeg_sys::demuxers() {
        println!("{name}");
    }
}
