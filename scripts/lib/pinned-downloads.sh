# Pinned downloads for scripts/build-php.sh: fetched, checked against a pinned sha256, and kept only on a match.
# Sourced, not run; crates/speedyspec-php/tests/build_script.rs tests each function.

# Prints the sha256 of file $1.
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# Downloads URL $1 to $3 if its sha256 is $2; $4 names it in messages. On a mismatch nothing is kept and the script
# exits 1.
fetch_pinned() {
    local url="$1" sha256="$2" dest="$3" name="$4" actual
    echo "build-php.sh: downloading $name"
    mkdir -p "$(dirname "$dest")"
    curl -fsSL -o "$dest.download" "$url"
    actual="$(sha256_of "$dest.download")"
    if [ "$actual" != "$sha256" ]; then
        rm -f "$dest.download"
        echo "build-php.sh: checksum mismatch for $name" >&2
        echo "  expected $sha256" >&2
        echo "  actual   $actual" >&2
        exit 1
    fi
    mv "$dest.download" "$dest"
}

# Exits 1 unless every libsodium tarball spc left in its downloads directory $1 is the pinned one (version $2, sha256
# $3): spc must have built from the tarball build-php.sh fetched, not one it fetched itself.
check_spc_libsodium() {
    local downloads="$1" version="$2" sha256="$3" tarball found=""
    for tarball in "$downloads"/libsodium-*.tar.gz; do
        [ -e "$tarball" ] || continue
        found=1
        if [ "$(sha256_of "$tarball")" != "$sha256" ]; then
            echo "build-php.sh: sodium: spc used $tarball, not the pinned libsodium $version" >&2
            echo "  remove $tarball and rerun scripts/build-php.sh" >&2
            exit 1
        fi
    done
    if [ -z "$found" ]; then
        echo "build-php.sh: sodium: spc left no libsodium tarball in $downloads, so the pinned libsodium $version can't be confirmed" >&2
        echo "  remove $(dirname "$downloads")/buildroot and rerun scripts/build-php.sh" >&2
        exit 1
    fi
}
