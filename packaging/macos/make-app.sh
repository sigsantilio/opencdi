#!/bin/sh
# Wraps a built OpenCDi binary in a macOS application bundle.
#
#   packaging/macos/make-app.sh <opencdi-binary> <output-dir> [version]
#
# The binary may be single-architecture or a universal one made with `lipo`. The bundle is
# ad-hoc signed, which Apple Silicon requires to launch it; it is not notarized.
set -eu

bin=${1:?usage: make-app.sh <opencdi-binary> <output-dir> [version]}
out=${2:?usage: make-app.sh <opencdi-binary> <output-dir> [version]}
root=$(cd "$(dirname "$0")/../.." && pwd)
version=${3:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)}

app="$out/OpenCDi.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/opencdi"
chmod +x "$app/Contents/MacOS/opencdi"

# Icon: icon.ico holds a 256x256 image; build the .icns sizes from it.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
sips -s format png "$root/icon.ico" --out "$work/icon.png" >/dev/null
iconset="$work/OpenCDi.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256; do
    sips -z $size $size "$work/icon.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    if [ $double -le 256 ]; then
        sips -z $double $double "$work/icon.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    fi
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/OpenCDi.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>OpenCDi</string>
  <key>CFBundleDisplayName</key><string>OpenCDi</string>
  <key>CFBundleIdentifier</key><string>io.github.sigsantilio.opencdi</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>opencdi</string>
  <key>CFBundleIconFile</key><string>OpenCDi</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --sign - "$app/Contents/MacOS/opencdi"
codesign --force --sign - "$app"
echo "$app"
