//! WordPress's media functions in a worker: read through the runtime's own `media-probe` child and the file cache.

mod support;

use std::io::{Read, Write};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_speedyspec-php-runtime");

/// A 1 s, 8 kHz, mono, 16-bit PCM WAV of silence.
fn wav() -> Vec<u8> {
    let data = 16_000u32;
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [
        16u32.to_le_bytes(),
        [1, 0, 1, 0],
        8000u32.to_le_bytes(),
        16_000u32.to_le_bytes(),
        [2, 0, 16, 0],
    ] {
        out.extend_from_slice(&v);
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    out.resize(44 + data as usize, 0);
    out
}

#[tokio::test(flavor = "current_thread")]
async fn wp_read_audio_metadata_in_a_worker() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tone.wav");
    std::fs::write(&file, wav()).unwrap();
    let site = dir.path().join("site");
    std::fs::create_dir_all(&site).unwrap();
    let app = support::release(
        dir.path(),
        "app",
        &format!(
            "<?php $m = wp_read_audio_metadata({:?}); echo json_encode([$m['length'] ?? null, $m['fileformat'] ?? null]);",
            file.display().to_string()
        ),
    );
    let mut worker = support::spawn(dir.path(), &app, Some(&site), &[]);
    worker.ready().await;
    let first = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(first.body(), r#"[1,"wav"]"#, "{}", first.stdout);
    // The second read is served from the cache under the site directory.
    let second = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(second.body(), r#"[1,"wav"]"#);
    assert!(
        site.join("cache").exists(),
        "the media cache lives under the site directory"
    );
}

/// An image type PHP can't size (JPEG XL) goes through the worker's own probe child; a PNG through PHP's getimagesize().
#[tokio::test(flavor = "current_thread")]
async fn wp_getimagesize_in_a_worker() {
    let dir = tempfile::tempdir().unwrap();
    let images = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/media/images");
    let site = dir.path().join("site");
    std::fs::create_dir_all(&site).unwrap();
    for name in ["photo.jxl", "rgb.png"] {
        std::fs::copy(images.join(name), site.join(name)).unwrap();
    }
    let app = support::release(
        dir.path(),
        "app",
        &format!(
            "<?php $j = wp_getimagesize({:?}); $p = wp_getimagesize({:?}); echo json_encode([$j[0] ?? null, $j['mime'] ?? null, $p[0] ?? null, $p['mime'] ?? null]);",
            site.join("photo.jxl").display().to_string(),
            site.join("rgb.png").display().to_string()
        ),
    );
    let mut worker = support::spawn(dir.path(), &app, Some(&site), &[]);
    worker.ready().await;
    let answer = worker.get(&app.join("index.php"), "/").await;
    assert_eq!(answer.body(), r#"[64,"image\/jxl",64,"image\/png"]"#, "{}", answer.stdout);
}

#[test]
fn media_probe_subcommand_reads_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tone.wav");
    std::fs::write(&file, wav()).unwrap();
    let mut child = Command::new(BIN)
        .args([
            "media-probe",
            "--max-bytes",
            "67108864",
            "--deadline-ms",
            "5000",
            "--max-alloc",
            "268435456",
            "--max-streams",
            "32",
            "--max-picture-bytes",
            "16777216",
            "--max-tag-bytes",
            "1048576",
        ])
        .stdin(std::fs::File::open(&file).unwrap())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut out).unwrap();
    assert!(child.wait().unwrap().success());
    let len = u32::from_le_bytes(out[..4].try_into().unwrap()) as usize;
    assert_eq!(out.len(), 4 + len);
    let _ = std::io::stderr().flush();
}
