#!/bin/sh
set -eu

ROOT="$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/mobile/android/app/src/main/jniLibs/arm64-v8a"
API=24

if [ -n "${ANDROID_NDK_HOME:-}" ]; then
    NDK="$ANDROID_NDK_HOME"
elif [ -n "${ANDROID_NDK_ROOT:-}" ]; then
    NDK="$ANDROID_NDK_ROOT"
elif [ -n "${ANDROID_HOME:-}" ] && [ -d "$ANDROID_HOME/ndk" ]; then
    NDK="$(ls -d "$ANDROID_HOME/ndk"/* 2>/dev/null | sort -V | tail -1)"
elif [ -d "$HOME/Library/Android/sdk/ndk" ]; then
    NDK="$(ls -d "$HOME/Library/Android/sdk/ndk"/* 2>/dev/null | sort -V | tail -1)"
else
    echo "Set ANDROID_NDK_HOME to an NDK install, then run this script again."
    exit 1
fi

case "$(uname -m)" in
    arm64|aarch64) HOST_TAG="darwin-arm64" ;;
    *) HOST_TAG="darwin-x86_64" ;;
esac

if [ "$(uname -s)" != "Darwin" ]; then
    case "$(uname -m)" in
        arm64|aarch64) HOST_TAG="linux-aarch64" ;;
        *) HOST_TAG="linux-x86_64" ;;
    esac
fi

BIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
if [ ! -x "$BIN/aarch64-linux-android${API}-clang" ] && [ -x "$NDK/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android${API}-clang" ]; then
    BIN="$NDK/toolchains/llvm/prebuilt/darwin-x86_64/bin"
fi

if [ ! -x "$BIN/aarch64-linux-android${API}-clang" ]; then
    echo "NDK clang was not found under $NDK"
    exit 1
fi

export PATH="$BIN:$PATH"
export CC_aarch64_linux_android="$BIN/aarch64-linux-android${API}-clang"
export CXX_aarch64_linux_android="$BIN/aarch64-linux-android${API}-clang++"
export AR_aarch64_linux_android="$BIN/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$BIN/aarch64-linux-android${API}-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$BIN/llvm-ar"

rustup target add aarch64-linux-android
cd "$ROOT"
cargo build -p base --target aarch64-linux-android --lib --release
mkdir -p "$OUT"
cp "$ROOT/target/aarch64-linux-android/release/libbase.so" "$OUT/libbase.so"
echo "Wrote $OUT/libbase.so"

if [ -x "$ROOT/mobile/android/gradlew" ]; then
    (cd "$ROOT/mobile/android" && ./gradlew assembleDebug)
elif command -v gradle >/dev/null 2>&1; then
    (cd "$ROOT/mobile/android" && gradle assembleDebug)
else
    echo "Open mobile/android in Android Studio to package the APK."
fi
