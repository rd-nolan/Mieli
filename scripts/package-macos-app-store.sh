#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
release_dir="$root_dir/target/release"
app_dir="$release_dir/Mieli.app"
pkg_path="${MIELI_PKG_PATH:-$release_dir/Mieli.pkg}"
binary="$release_dir/mieli"
entitlements="$root_dir/resources/Mieli.entitlements"
info_plist="$root_dir/resources/Info.plist"

app_sign_identity="${MIELI_APP_SIGN_IDENTITY:-}"
installer_sign_identity="${MIELI_INSTALLER_SIGN_IDENTITY:-}"
profile_path="${MIELI_PROVISIONING_PROFILE:-}"
version="${MIELI_VERSION:-}"
build_number="${MIELI_BUILD_NUMBER:-}"

if [[ -z "$app_sign_identity" ]]; then
	printf 'MIELI_APP_SIGN_IDENTITY is required\n' >&2
	exit 1
fi

if [[ -z "$installer_sign_identity" ]]; then
	printf 'MIELI_INSTALLER_SIGN_IDENTITY is required\n' >&2
	exit 1
fi

if [[ -z "$profile_path" || ! -f "$profile_path" ]]; then
	printf 'MIELI_PROVISIONING_PROFILE must point to an existing .mobileprovision file\n' >&2
	exit 1
fi

if [[ -z "$version" ]]; then
	printf 'MIELI_VERSION is required\n' >&2
	exit 1
fi

if [[ -z "$build_number" ]]; then
	printf 'MIELI_BUILD_NUMBER is required and must be unique for the App Store version\n' >&2
	exit 1
fi

if [[ ! "$version" =~ ^[0-9]+([.][0-9]+){0,2}$ ]]; then
	printf 'MIELI_VERSION must contain one to three dot-separated numeric components\n' >&2
	exit 1
fi

if [[ ! "$build_number" =~ ^[0-9]+([.][0-9]+){0,2}$ ]]; then
	printf 'MIELI_BUILD_NUMBER must contain one to three dot-separated numeric components\n' >&2
	exit 1
fi

cargo build --manifest-path "$root_dir/Cargo.toml" --release

if [[ ! -x "$binary" ]]; then
	printf 'release binary was not produced: %s\n' "$binary" >&2
	exit 1
fi

rm -rf "$app_dir" "$pkg_path"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"

cp "$binary" "$app_dir/Contents/MacOS/Mieli"
cp "$root_dir/resources/mieli.icns" "$app_dir/Contents/Resources/mieli.icns"
cp "$root_dir/resources/mieli_logo_1024x1024.png" "$app_dir/Contents/Resources/mieli_logo_1024x1024.png"
cp "$info_plist" "$app_dir/Contents/Info.plist"
cp "$profile_path" "$app_dir/Contents/embedded.provisionprofile"

/usr/bin/plutil -replace CFBundleShortVersionString -string "$version" "$app_dir/Contents/Info.plist"
/usr/bin/plutil -replace CFBundleVersion -string "$build_number" "$app_dir/Contents/Info.plist"
/usr/bin/plutil -lint "$app_dir/Contents/Info.plist" >/dev/null

/usr/bin/codesign \
	--force \
	--timestamp \
	--entitlements "$entitlements" \
	--sign "$app_sign_identity" \
	"$app_dir"

/usr/bin/codesign --verify --deep --strict --verbose=2 "$app_dir"

/usr/bin/productbuild \
	--sign "$installer_sign_identity" \
	--component "$app_dir" /Applications \
	"$pkg_path"

/usr/sbin/pkgutil --check-signature "$pkg_path"

echo "Created signed App Store package: $pkg_path"
