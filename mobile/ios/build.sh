#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)"
IOS="$ROOT/mobile/ios"
MODE="${1:-app}"
PLATFORM_NAME="${PLATFORM_NAME:-iphoneos}"

if [ "$MODE" = "simulator" ]; then
    PLATFORM_NAME="iphonesimulator"
    MODE="app"
fi

if [ "$PLATFORM_NAME" = "iphonesimulator" ]; then
    SDK="iphonesimulator"
    if [ "$(uname -m)" = "x86_64" ]; then
        TARGET="x86_64-apple-ios"
        ARCH="x86_64"
    else
        TARGET="aarch64-apple-ios-sim"
        ARCH="arm64"
    fi
else
    SDK="iphoneos"
    TARGET="aarch64-apple-ios"
    ARCH="arm64"
fi

export PATH="$IOS/bin:$PATH"
unset IPHONEOS_DEPLOYMENT_TARGET
unset SDKROOT

if ! xcrun --sdk "$SDK" --show-sdk-path >/dev/null 2>&1; then
    echo "The $SDK SDK is not installed. Install Xcode, then run this script again."
    exit 1
fi

SDKROOT="$(xcrun --sdk "$SDK" --show-sdk-path)"
mkdir -p "$IOS/build"
cd "$ROOT"
rustup target add "$TARGET"
if ! NOTES="$(cargo rustc -p base --target "$TARGET" --lib --release -- --print native-static-libs 2>&1)"; then
    printf '%s\n' "$NOTES"
    exit 1
fi
printf '%s\n' "$NOTES"
cp "$ROOT/target/$TARGET/release/libbase.a" "$IOS/build/libbase.a"

if [ "$MODE" = "lib" ]; then
    echo "Wrote $IOS/build/libbase.a"
    exit 0
fi

NATIVE="$(printf '%s\n' "$NOTES" | sed -n 's/.*native-static-libs: //p' | tail -1)"
xcrun -sdk "$SDK" clang \
    -arch "$ARCH" \
    -isysroot "$SDKROOT" \
    -mios-version-min=14.0 \
    -fobjc-arc \
    "$IOS/Engine/main.m" \
    -force_load "$IOS/build/libbase.a" \
    $NATIVE \
    -framework UIKit \
    -framework Metal \
    -framework QuartzCore \
    -framework Foundation \
    -framework CoreGraphics \
    -framework CoreFoundation \
    -framework Security \
    -o "$IOS/build/Engine"

APP="$IOS/build/Engine.app"
rm -rf "$APP"
mkdir -p "$APP"
cp "$IOS/build/Engine" "$APP/Engine"
cp "$IOS/Engine/Info.plist" "$APP/Info.plist"
codesign --force --sign - "$APP" >/dev/null 2>&1 || true
echo "Wrote $APP"
