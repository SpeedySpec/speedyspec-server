# Licensing

speedyspec-server is MIT-licensed. The PHP runtime (`speedyspec-php-runtime`) also links third-party libraries
statically; this page covers the one whose licence asks something of every release: FFmpeg.

## FFmpeg

The runtime reads audio, video and image metadata with FFmpeg (`libavformat`, `libavcodec`, `libavutil`), built by
`scripts/build-ffmpeg.sh` and linked statically (`docs/php.md`, "FFmpeg").

- **Licence.** FFmpeg as configured is under the GNU Lesser General Public License, version 2.1 or later. The build
  never enables GPL, version 3 or nonfree parts: the script fails if `CONFIG_GPL`, `CONFIG_VERSION3` or
  `CONFIG_NONFREE` is set, and `speedyspec-ffmpeg-sys`'s tests check that FFmpeg reports "LGPL version 2.1 or later".
- **Relinking (LGPL 2.1 §6).** A static link must let the recipient relink the work against a modified library. The
  runtime's complete source, with the scripts that build and link FFmpeg, is public at every release tag, so anyone can
  rebuild it against their own FFmpeg (§6(a)). Nothing in the runtime's MIT terms restricts modification or reverse
  engineering.
- **What every runtime release attaches.** `ffmpeg-<version>.tar.xz` (the exact source), `ffmpeg-configure-line.txt`,
  `COPYING.LGPLv2.1` and `THIRD-PARTY-NOTICES.md`, made by `scripts/ffmpeg-licence-bundle.sh` in `release.yml`. The
  source is attached rather than offered in writing, so the attachments stay for as long as the release is offered.
  The publish job fails a runtime release without them, and `scripts/release-smoke.sh` fails a runtime whose manifest
  doesn't report the pinned FFmpeg version.
- **Distribution channels.** The runtime ships only as GitHub release binaries. The server's container image and the
  Kubernetes manifests carry no runtime, and `speedyspec-ffmpeg-sys` is not published to crates.io.

### Relinking FFmpeg

1. Check out the release tag of `speedyspec-php-runtime` you received.
2. Build the static PHP: `scripts/build-php.sh` (or `scripts/linux-php.sh scripts/build-php.sh` for Linux musl). This
   also builds the pinned FFmpeg into `.php-build/ffmpeg/prefix`.
3. Build your modified FFmpeg with the arguments in `ffmpeg-configure-line.txt` (or
   `scripts/build-ffmpeg.sh --print-configure`) and `--prefix` pointing at a directory of your choice, then set
   `FFMPEG_PREFIX` to that directory.
4. `cargo build --release --locked -p speedyspec-php-runtime`. The binary in `target/release/` links your FFmpeg.
