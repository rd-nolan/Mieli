#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
app_dir="$root_dir/target/release/Mieli.app"
binary="$root_dir/target/release/mieli"

cargo build --manifest-path "$root_dir/Cargo.toml" --release

if [[ ! -x "$binary" ]]; then
    echo "release binary was not produced: $binary" >&2
    exit 1
fi

rm -rf "$app_dir"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
cp "$binary" "$app_dir/Contents/MacOS/Mieli"
cp "$root_dir/resources/mieli.icns" "$app_dir/Contents/Resources/mieli.icns"
cp "$root_dir/resources/mieli_logo_1024x1024.png" "$app_dir/Contents/Resources/mieli_logo_1024x1024.png"
cp "$root_dir/resources/Info.plist" "$app_dir/Contents/Info.plist"

/usr/bin/plutil -lint "$app_dir/Contents/Info.plist" >/dev/null
echo "Created $app_dir"
