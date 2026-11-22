#!/usr/bin/env bash
# Generates compat/media/images: small images in every format WordPress's image functions or FFmpeg read, plus broken
# ones, all from synthetic sources (FFmpeg's testsrc2 frames). Runs one throwaway Alpine container with FFmpeg,
# ImageMagick, libheif, libjxl and ExifTool. PROVENANCE.md lists each file's command; rerun to regenerate.
#
#   compat/media/make-image-corpus.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$here/images"
mkdir -p "$out"
rm -f "$out"/*

docker run --rm -v "$out:/out" -w /out alpine:3.22 sh -euc '
apk add -q --no-cache ffmpeg imagemagick imagemagick-heic imagemagick-jxl imagemagick-tiff libheif-tools libjxl-tools exiftool >/dev/null
F="ffmpeg -nostdin -hide_banner -loglevel error -y"
SRC="-f lavfi -i testsrc2=size=64x48:duration=1"
SRCA="-f lavfi -i testsrc2=size=64x48:duration=1,format=rgba,geq=r=r(X\,Y):g=g(X\,Y):b=b(X\,Y):a=128"
ANIM="-f lavfi -i testsrc2=size=64x48:rate=3:duration=1"

$F $SRC -frames:v 1 src.png

# JPEG.
$F $SRC -frames:v 1 baseline.jpg
magick src.png -interlace JPEG progressive.jpg
magick src.png -colorspace Gray gray.jpg
magick src.png -colorspace CMYK cmyk.jpg
cp baseline.jpg iptc.jpg && exiftool -q -overwrite_original -IPTC:Keywords=corpus -IPTC:Caption-Abstract=caption iptc.jpg
cp baseline.jpg exif-rotated.jpg && exiftool -q -overwrite_original -Orientation=6 -n exif-rotated.jpg

# PNG.
cp src.png rgb.png
$F $SRCA -frames:v 1 rgba.png
$F $SRC -frames:v 1 -pix_fmt pal8 palette.png
$F $SRC -frames:v 1 -pix_fmt gray16be gray16.png
$F $ANIM -plays 0 -f apng animated.png

# GIF, BMP, TIFF.
$F $SRC -frames:v 1 static.gif
$F $ANIM animated.gif
$F $SRC -frames:v 1 -pix_fmt bgr24 rgb24.bmp
$F $SRCA -frames:v 1 -pix_fmt bgra rgba32.bmp
$F $SRC -frames:v 1 -pix_fmt pal8 pal8.bmp
$F $SRC -frames:v 1 rgb.tif
magick src.png -colorspace CMYK cmyk.tif

# WebP.
$F $SRC -frames:v 1 -c:v libwebp -lossless 0 lossy.webp
$F $SRC -frames:v 1 -c:v libwebp -lossless 1 lossless.webp
$F $SRCA -frames:v 1 -c:v libwebp -lossless 0 alpha.webp
$F $ANIM -c:v libwebp -loop 0 animated.webp

# AVIF, HEIC, JPEG XL.
magick src.png -quality 50 lossy.avif
$F $SRCA -frames:v 1 -c:v png rgba-src.png && magick rgba-src.png -quality 50 alpha.avif && rm rgba-src.png
($F $ANIM -c:v libaom-av1 -cpu-used 8 -crf 40 animated.avif 2>/dev/null || $F $ANIM -c:v libsvtav1 animated.avif) || true
heif-enc -q 50 -o photo.heic src.png >/dev/null
cjxl -q 80 src.png photo.jxl >/dev/null 2>&1

# ICO and others FFmpeg reads.
magick src.png -define icon:auto-resize=32,16 icon.ico
$F $SRC -frames:v 1 photo.qoi
$F $SRC -frames:v 1 photo.pcx
$F $SRC -frames:v 1 photo.tga
$F $SRC -frames:v 1 photo.ppm
$F $SRC -frames:v 1 -pix_fmt gray photo.pgm
$F $SRC -frames:v 1 photo.sgi
$F $SRC -frames:v 1 -c:v jpeg2000 -f image2 photo.jp2
magick src.png photo.psd
$F $SRC -frames:v 1 photo.hdr

# Broken.
head -c 200 baseline.jpg > truncated.jpg
: > empty.png
cp rgb.png mislabelled.jpg

rm src.png
ls -la | awk "{s+=\$5} END {print s, \"bytes\"}"
'
du -sh "$out"; ls "$out" | wc -l
