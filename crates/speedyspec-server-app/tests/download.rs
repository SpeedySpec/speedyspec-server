//! `sha256_file` against digests from `shasum -a 256`.

use speedyspec_server_app::download::sha256_file;

#[test]
fn sha256_file_matches_shasum() {
    let dir = tempfile::tempdir().unwrap();
    let small = dir.path().join("abc");
    std::fs::write(&small, b"abc").unwrap();
    assert_eq!(
        sha256_file(&small).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    // Over one 64 KiB read, so the digest spans chunks.
    let large = dir.path().join("large");
    std::fs::write(&large, vec![b'x'; 100_000]).unwrap();
    assert_eq!(
        sha256_file(&large).unwrap(),
        "d69e68988157833272305aaf21f453c800346e8a3640db6578e260215542e5d4"
    );
}

#[test]
fn sha256_file_missing_is_an_error() {
    assert!(sha256_file(std::path::Path::new("/nonexistent/speedyspec/file.zip")).is_err());
}
