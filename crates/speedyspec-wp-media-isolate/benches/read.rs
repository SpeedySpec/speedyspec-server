//! What reading a file costs: FFmpeg in-process, through the probe child, and from the cache (local and shared).

use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use speedyspec_cache::CachingStore;
use speedyspec_cache_memory::MemoryCache;
use speedyspec_wp_media_contracts::{MediaReader, MediaSource, ProbeLimits};
use speedyspec_wp_media_ffmpeg::FfmpegReader;
use speedyspec_wp_media_isolate::{CachedReader, IsolatedReader, ProbeCommand};
use speedyspec_wp_media_testkit as kit;

fn isolated() -> IsolatedReader {
    IsolatedReader::new(ProbeCommand {
        program: PathBuf::from(env!("CARGO_BIN_EXE_speedyspec-media-probe")),
        args: Vec::<OsString>::new(),
    })
}

fn bench(c: &mut Criterion) {
    let limits = ProbeLimits::default();
    let png = kit::png(640, 480);
    let fixtures = [
        ("png 640x480", kit::write_temp(&png, "png")),
        ("mp3 id3v2+cover", kit::write_temp(&kit::mp3_with_id3v2(2000, "T", "A", Some(&kit::png(300, 300))), "mp3")),
        ("wav 30s", kit::write_temp(&kit::wav_pcm16(30.0, 44100, 2), "wav")),
    ];
    let mut group = c.benchmark_group("media read");
    group.measurement_time(Duration::from_secs(3)).sample_size(30);
    for (name, file) in &fixtures {
        let path = file.path().to_path_buf();
        let root = path.parent().unwrap().to_path_buf();
        let ffmpeg = FfmpegReader::new(&limits);
        group.bench_function(format!("{name}: in-process"), |b| {
            b.iter(|| ffmpeg.read(MediaSource::Path(path.clone()), &limits).unwrap())
        });
        let child = isolated();
        group.bench_function(format!("{name}: isolated, cold"), |b| {
            b.iter(|| child.read(MediaSource::Path(path.clone()), &limits).unwrap())
        });
        let store: Rc<dyn CachingStore> = Rc::new(MemoryCache::new());
        let warm = CachedReader::new(isolated(), store.clone(), root.clone(), Duration::from_secs(3600), NonZeroUsize::new(8).unwrap());
        warm.read(MediaSource::Path(path.clone()), &limits).unwrap();
        group.bench_function(format!("{name}: cached, local hit"), |b| {
            b.iter(|| warm.read(MediaSource::Path(path.clone()), &limits).unwrap())
        });
        group.bench_function(format!("{name}: cached, shared-store hit"), |b| {
            b.iter_batched(
                || CachedReader::new(isolated(), store.clone(), root.clone(), Duration::from_secs(3600), NonZeroUsize::new(8).unwrap()),
                |fresh| fresh.read(MediaSource::Path(path.clone()), &limits).unwrap(),
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
