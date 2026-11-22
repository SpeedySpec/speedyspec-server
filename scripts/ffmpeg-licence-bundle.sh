#!/usr/bin/env bash
# Writes the files every runtime release attaches for the statically linked FFmpeg (LGPL 2.1; docs/licensing.md):
#
#   ffmpeg-<version>.tar.xz     the exact source the runtime was built from (pinned in build-ffmpeg.sh)
#   ffmpeg-configure-line.txt   how it was configured
#   COPYING.LGPLv2.1            FFmpeg's licence, from that source
#   THIRD-PARTY-NOTICES.md      what is linked, under which licence, and where the runtime's source is
#
#   scripts/ffmpeg-licence-bundle.sh <out-dir> <runtime-source-url>
#
# No name starts with a component name, so release.yml's "$COMPONENT"-* globs and release-index.sh ignore them.
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: scripts/ffmpeg-licence-bundle.sh <out-dir> <runtime-source-url>" >&2
    exit 2
fi
out="$1" source_url="$2"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# shellcheck source=lib/pinned-downloads.sh
source "$repo/scripts/lib/pinned-downloads.sh"
pin() { sed -n "s/^$1=\"\(.*\)\"$/\1/p" "$repo/scripts/build-ffmpeg.sh"; }
version="$(pin ffmpeg_version)"
sha256="$(pin ffmpeg_sha256)"
url="$(pin ffmpeg_url | sed "s/\$ffmpeg_version/$version/")"
[ -n "$version" ] && [ -n "$sha256" ] && [ -n "$url" ] \
    || { echo "ffmpeg-licence-bundle.sh: build-ffmpeg.sh has no complete pin" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$out" "$work/ffmpeg/downloads"
tarball="$out/ffmpeg-$version.tar.xz"
if [ ! -f "$tarball" ] || [ "$(sha256_of "$tarball")" != "$sha256" ]; then
    fetch_pinned "$url" "$sha256" "$tarball" "ffmpeg-$version.tar.xz"
fi
cp "$tarball" "$work/ffmpeg/downloads/"

# The PHP buildroot path differs per build machine; the line names it as a placeholder.
{
    echo "# FFmpeg $version configure arguments, one per line. <php-buildroot> is the static PHP's buildroot"
    echo "# (.php-build/buildroot); macOS builds add -mmacosx-version-min=12.0 to both --extra flags."
    SPEEDYSPEC_PHP_PREFIX="<php-buildroot>" SPEEDYSPEC_FFMPEG_DIR="$work/ffmpeg" \
        "$repo/scripts/build-ffmpeg.sh" --print-configure 2>/dev/null
} > "$out/ffmpeg-configure-line.txt"

tar -xJOf "$tarball" "ffmpeg-$version/COPYING.LGPLv2.1" > "$out/COPYING.LGPLv2.1"

sed -e "s|@FFMPEG_VERSION@|$version|g" -e "s|@FFMPEG_URL@|$url|g" -e "s|@FFMPEG_SHA256@|$sha256|g" \
    -e "s|@RUNTIME_SOURCE@|$source_url|g" "$repo/docs/THIRD-PARTY-NOTICES.md.in" > "$out/THIRD-PARTY-NOTICES.md"

echo "ffmpeg-licence-bundle.sh: wrote FFmpeg $version's licence attachments to $out"
