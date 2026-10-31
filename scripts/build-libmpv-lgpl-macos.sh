#!/usr/bin/env bash
# macOS (Apple Silicon) counterpart of build-libmpv-lgpl.sh: builds an LGPL libmpv for the
# embedded player of the macOS build, with its own LGPL FFmpeg (VideoToolbox hardware decoding),
# libplacebo (OpenGL only), libass (CoreText fonts), FreeType, FriBidi, HarfBuzz and dav1d.
#
#   scripts/build-libmpv-lgpl-macos.sh      full build into $ANIX_LIBMPV_DIR/out/Frameworks
#
# Nothing GPL goes in: FFmpeg is configured without --enable-gpl/--enable-nonfree and with
# --disable-autodetect, mpv with -Dgpl=false; the result is checked the same way as on Linux.
# Every library is built here (none from Homebrew) for macOS 11 and later, with the install name
# @rpath/<file> and the run path @loader_path, so the set works from any folder: the app bundle
# keeps it in Contents/Frameworks (see tauri.macos.conf.json).
#
# Needs: Xcode command line tools, Homebrew's meson, ninja, nasm, pkgconf, and python3 with
# jinja2 importable by meson's python (libplacebo generates its OpenGL loader with it).
set -euo pipefail

root="${ANIX_LIBMPV_DIR:-$HOME/.cache/anix-build/libmpv-macos}"
case "$root" in *" "*) echo "ANIX_LIBMPV_DIR must not contain spaces: $root" >&2; exit 1 ;; esac
jobs="${JOBS:-$(sysctl -n hw.ncpu)}"
src="$root/src"
out="$root/out"
mkdir -p "$src" "$root/prefix"
prefix="/tmp/anix-libmpv-prefix-$(id -u)"
ln -sfn "$root/prefix" "$prefix"

FREETYPE_VERSION=2.13.3
FRIBIDI_VERSION=1.0.16
HARFBUZZ_VERSION=11.2.1
LIBASS_VERSION=0.17.4
DAV1D_VERSION=1.5.1
LIBPLACEBO_TAG=v7.351.0
FFMPEG_VERSION=8.0.1
MPV_TAG=v0.41.0

export MACOSX_DEPLOYMENT_TARGET=11.0
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig"
# Keep build paths out of the binaries (they would carry the user name).
export CFLAGS="-O2 -arch arm64 -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET -ffile-prefix-map=$root=/build"
export CXXFLAGS="$CFLAGS"
export OBJCFLAGS="$CFLAGS"
export LDFLAGS="-arch arm64 -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET -Wl,-headerpad_max_install_names"

fetch() { # url file
  [ -f "$src/$2" ] || { curl -fL --retry 5 --retry-all-errors -o "$src/$2.part" "$1" && mv "$src/$2.part" "$src/$2"; }
}
unpack() { # file dir
  [ -d "$src/$2" ] || { mkdir -p "$src/$2" && tar -xf "$src/$1" -C "$src/$2" --strip-components=1; }
}
clone() { # url tag dir
  [ -d "$src/$3" ] || git clone --depth 1 --recurse-submodules --shallow-submodules --branch "$2" "$1" "$src/$3"
}
step() { echo; echo "==> $*"; }
meson_lib() { # dir, options...
  local dir="$1"; shift
  meson setup --reconfigure "$dir/build" "$dir" --prefix="$prefix" --libdir=lib --buildtype=release \
    -Ddefault_library=shared -Db_ndebug=true "$@"
  ninja -C "$dir/build" -j"$jobs"
  meson install -C "$dir/build"
}
t0=$(date +%s)

# --- FreeType (FTL), FriBidi (LGPL-2.1+), HarfBuzz (MIT): libass's text stack ---------------
step "FreeType $FREETYPE_VERSION"
fetch "https://download.savannah.gnu.org/releases/freetype/freetype-$FREETYPE_VERSION.tar.xz" freetype.tar.xz
unpack freetype.tar.xz freetype
meson_lib "$src/freetype" -Dharfbuzz=disabled -Dpng=disabled -Dbrotli=disabled -Dbzip2=disabled -Dzlib=system \
  -Dtests=disabled

step "FriBidi $FRIBIDI_VERSION"
fetch "https://github.com/fribidi/fribidi/releases/download/v$FRIBIDI_VERSION/fribidi-$FRIBIDI_VERSION.tar.xz" fribidi.tar.xz
unpack fribidi.tar.xz fribidi
meson_lib "$src/fribidi" -Ddocs=false -Dbin=false -Dtests=false

step "HarfBuzz $HARFBUZZ_VERSION"
fetch "https://github.com/harfbuzz/harfbuzz/releases/download/$HARFBUZZ_VERSION/harfbuzz-$HARFBUZZ_VERSION.tar.xz" harfbuzz.tar.xz
unpack harfbuzz.tar.xz harfbuzz
meson_lib "$src/harfbuzz" -Dfreetype=enabled -Dglib=disabled -Dgobject=disabled -Dcairo=disabled -Dchafa=disabled \
  -Dicu=disabled -Dgraphite2=disabled -Dcoretext=disabled -Dtests=disabled -Ddocs=disabled -Dutilities=disabled \
  -Dintrospection=disabled

# --- libass (ISC), with CoreText for system fonts ------------------------------------------
step "libass $LIBASS_VERSION"
fetch "https://github.com/libass/libass/releases/download/$LIBASS_VERSION/libass-$LIBASS_VERSION.tar.xz" libass.tar.xz
unpack libass.tar.xz libass
(cd "$src/libass" && ./configure --prefix="$prefix" --disable-static --enable-shared \
  --disable-fontconfig --enable-coretext --disable-libunibreak --disable-require-system-font-provider)
make -C "$src/libass" -j"$jobs" && make -C "$src/libass" install

# --- dav1d (BSD-2): AV1 decoding --------------------------------------------------------
step "dav1d $DAV1D_VERSION"
clone https://code.videolan.org/videolan/dav1d.git "$DAV1D_VERSION" dav1d
meson_lib "$src/dav1d" -Denable_tools=false -Denable_tests=false -Denable_examples=false

# --- libplacebo (LGPL-2.1+), OpenGL only ---------------------------------------------------
step "libplacebo $LIBPLACEBO_TAG"
clone https://code.videolan.org/videolan/libplacebo.git "$LIBPLACEBO_TAG" libplacebo
meson_lib "$src/libplacebo" -Dvulkan=disabled -Dshaderc=disabled -Dglslang=disabled -Dd3d11=disabled \
  -Dopengl=enabled -Dlcms=disabled -Dlibdovi=disabled -Dxxhash=disabled -Ddemos=false -Dtests=false

# --- FFmpeg (LGPL-2.1+), with VideoToolbox --------------------------------------------------
step "FFmpeg $FFMPEG_VERSION"
fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" ffmpeg.tar.xz
unpack ffmpeg.tar.xz ffmpeg
# Network protocols stay (mpv reads from Anix's loopback proxy); TLS is not needed for that.
(cd "$src/ffmpeg" && ./configure --prefix="$prefix" --libdir="$prefix/lib" --install-name-dir='@rpath' \
  --arch=arm64 --cc=clang --extra-cflags="$CFLAGS" --extra-ldflags="$LDFLAGS" \
  --enable-shared --disable-static --enable-pic --disable-programs --disable-doc --disable-debug \
  --disable-autodetect --enable-zlib --enable-videotoolbox --enable-audiotoolbox --enable-libdav1d)
grep -q '^#define CONFIG_GPL 0' "$src/ffmpeg/config.h" || { echo "FFmpeg is configured as GPL; refusing." >&2; exit 1; }
grep -q '^#define CONFIG_NONFREE 0' "$src/ffmpeg/config.h" || { echo "FFmpeg is configured as non-free; refusing." >&2; exit 1; }
grep -q 'FFMPEG_LICENSE "LGPL version 2.1 or later"' "$src/ffmpeg/config.h" || { echo "FFmpeg is not LGPL 2.1+; refusing." >&2; exit 1; }
make -C "$src/ffmpeg" -j"$jobs" && make -C "$src/ffmpeg" install

# --- mpv (LGPL-2.1+ with -Dgpl=false), Objective-C Cocoa without Swift ----------------------
step "mpv $MPV_TAG"
clone https://github.com/mpv-player/mpv.git "$MPV_TAG" mpv
meson_lib "$src/mpv" -Dgpl=false -Dlibmpv=true -Dcplayer=false \
  -Dlua=disabled -Djavascript=disabled -Dlibarchive=disabled -Dlibbluray=disabled -Ddvdnav=disabled \
  -Dcdda=disabled -Duchardet=disabled -Drubberband=disabled -Dvapoursynth=disabled -Dzimg=disabled \
  -Dlcms2=disabled -Djpeg=disabled -Dvulkan=disabled -Dx11=disabled -Dwayland=disabled -Degl=disabled \
  -Dcocoa=enabled -Dgl=enabled -Dgl-cocoa=enabled -Dvideotoolbox-gl=enabled -Dvideotoolbox-pl=disabled \
  -Dswift-build=disabled -Dmacos-cocoa-cb=disabled -Dmacos-media-player=disabled -Dmacos-touchbar=disabled \
  -Dcoreaudio=enabled -Davfoundation=disabled -Daudiounit=disabled \
  -Dmanpage-build=disabled -Dhtml-build=disabled -Dpdf-build=disabled

# --- collect and check --------------------------------------------------------------------
step "collect into $out/Frameworks"
rm -rf "$out"; mkdir -p "$out/Frameworks" "$out/licenses"
for lib in libmpv libavcodec libavformat libavutil libavfilter libavdevice libswresample libswscale \
  libplacebo libass libfreetype libfribidi libharfbuzz libdav1d; do
  # Each library once, under its install name (FFmpeg's real file is libavcodec.62.11.100.dylib,
  # but it and its users call it libavcodec.62.dylib).
  for f in "$prefix"/lib/"$lib".*dylib; do
    [ -e "$f" ] || continue
    name="$(basename "$(otool -D "$f" | tail -n 1)")"
    [ -e "$out/Frameworks/$name" ] || cp -L "$f" "$out/Frameworks/$name"
  done
done
for f in "$out"/Frameworks/*.dylib; do
  name="$(basename "$f")"
  install_name_tool -id "@rpath/$name" "$f"
  # Point every reference into the prefix (whatever the referenced file is called) at @rpath.
  otool -L "$f" | awk 'NR > 1 { print $1 }' | while read -r dep; do
    case "$dep" in
      "$prefix"/*|"$root"/*|@rpath/*)
        base="$(basename "$dep")"
        if [ "$dep" != "@rpath/$base" ]; then install_name_tool -change "$dep" "@rpath/$base" "$f"; fi
        ;;
    esac
  done
  install_name_tool -add_rpath '@loader_path' "$f" 2>/dev/null || true
  strip -x "$f"
  # Changing a Mach-O voids its signature; Apple Silicon refuses to load unsigned code.
  codesign --force --sign - "$f"
done
# Every library must find what it needs inside the set.
for f in "$out"/Frameworks/*.dylib; do
  for dep in $(otool -L "$f" | awk 'NR > 1 { print $1 }' | grep '^@rpath/' || true); do
    [ -e "$out/Frameworks/${dep#@rpath/}" ] || { echo "$(basename "$f") needs ${dep#@rpath/}, which is not in the set." >&2; exit 1; }
  done
  if otool -L "$f" | awk 'NR > 1 { print $1 }' | grep -vE '^(@rpath/|/usr/lib/|/System/Library/)'; then
    echo "$(basename "$f") links a library outside the set and the system (above)." >&2; exit 1
  fi
done

cp "$src/ffmpeg/COPYING.LGPLv2.1" "$out/licenses/FFmpeg-LGPL-2.1.txt"
cp "$src/mpv/LICENSE.LGPL" "$out/licenses/mpv-LGPL-2.1.txt"
cp "$src/libplacebo/LICENSE" "$out/licenses/libplacebo-LGPL-2.1.txt"
cp "$src/libass/COPYING" "$out/licenses/libass-ISC.txt"
cp "$src/freetype/docs/FTL.TXT" "$out/licenses/FreeType-FTL.txt"
cp "$src/fribidi/COPYING" "$out/licenses/FriBidi-LGPL-2.1.txt"
cp "$src/harfbuzz/COPYING" "$out/licenses/HarfBuzz-MIT.txt"
cp "$src/dav1d/COPYING" "$out/licenses/dav1d-BSD-2.txt"

python3 -I - "$out/Frameworks" <<'PY'
import ctypes, glob, os, sys
d = sys.argv[1]
av = ctypes.CDLL(sorted(glob.glob(f"{d}/libavcodec.*.dylib"))[0])
av.avcodec_license.restype = ctypes.c_char_p
license = av.avcodec_license().decode()
mpv = ctypes.CDLL(sorted(glob.glob(f"{d}/libmpv.*.dylib"))[0])
mpv.mpv_create.restype = ctypes.c_void_p
mpv.mpv_get_property_string.restype = ctypes.c_void_p
mpv.mpv_get_property_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
mpv.mpv_initialize.argtypes = [ctypes.c_void_p]
mpv.mpv_set_option_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]
mpv.mpv_terminate_destroy.argtypes = [ctypes.c_void_p]
h = mpv.mpv_create()
mpv.mpv_set_option_string(h, b"vo", b"null"); mpv.mpv_set_option_string(h, b"ao", b"null")
assert mpv.mpv_initialize(h) >= 0, "mpv_initialize failed"
config = ctypes.string_at(mpv.mpv_get_property_string(h, b"mpv-configuration")).decode()
mpv.mpv_terminate_destroy(h)
print("FFmpeg licence:", license)
print("mpv configuration:", config)
if "LGPL" not in license or "gpl=false" not in config.replace(" ", ""):
    sys.exit("Not an LGPL build; refusing.")
open(f"{os.path.dirname(d)}/licenses/BUILD-LICENSE.txt", "w").write(f"FFmpeg: {license}\nmpv: {config}\n")
PY

check_user="${ANIX_CHECK_USER-$(id -un)}"
if grep -rla -e "$HOME" ${check_user:+-e "$check_user"} "$out"; then
  echo "The libraries contain this machine's home folder or user name (files above)." >&2; exit 1
fi
du -sh "$out/Frameworks"
echo; echo "Done in $(( $(date +%s) - t0 ))s: $out/Frameworks"
