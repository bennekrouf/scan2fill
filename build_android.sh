#!/bin/bash
# Written by ShipScreen before each build — edits here are replaced.

# Exit on error
set -e

export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk"
NDK_VERSION=$(ls -1 "$ANDROID_NDK_HOME" 2>/dev/null | grep -E '^[0-9]+\.' | sort -V | tail -1)
export ANDROID_NDK_HOME="$ANDROID_NDK_HOME/$NDK_VERSION"

echo "=== Building Android APK ==="
echo "NDK: $NDK_VERSION"
# The project's own Dioxus CLI, if it keeps one in .tools, comes before PATH's.
if [ -x ".tools/bin/dx" ]; then
    export PATH="$PWD/.tools/bin:$PATH"
fi
if ! command -v dx &>/dev/null; then
    echo "❌ Dioxus CLI (dx) not found — install it with: cargo install dioxus-cli --locked"
    exit 1
fi
DX_VERSION=$(dx --version 2>/dev/null | awk '$1 == "dioxus" { print $2 }')
if [ -z "$DX_VERSION" ]; then
    echo "❌ The dx found ($(command -v dx)) is not the Dioxus CLI — install it with: cargo install dioxus-cli --locked"
    exit 1
fi
# dx refuses to bundle a project whose Dioxus library is a different version.
LIB_VERSION=$(awk '/^name = "dioxus"$/ { getline; gsub(/version = |"/, ""); print; exit }' Cargo.lock 2>/dev/null)
if [ -n "$LIB_VERSION" ] && [ "$DX_VERSION" != "$LIB_VERSION" ]; then
    echo "❌ dx is $DX_VERSION ($(command -v dx)) but this project uses dioxus $LIB_VERSION — they must match."
    echo "   Either move the project to dx's version:  cargo update -p dioxus --precise $DX_VERSION"
    echo "   or install the matching dx into it:       cargo install dioxus-cli --version $LIB_VERSION --locked --root .tools"
    exit 1
fi
echo "dx: $DX_VERSION ($(command -v dx))"
echo

if ! dx bundle --platform android --release; then
    echo "⚠️ dx bundle failed. This is expected due to duplicate icons."
    echo "Attempting to resolve duplicate resources..."
fi

TARGET_RES="target/dx/scan2fil/release/android/app/app/src/main/res"
SOURCE_ICONS="assets/icons/android"

if [ -d "$TARGET_RES" ] && [ -d "$SOURCE_ICONS" ]; then
    echo "🧹 Cleaning up ALL existing launcher icons in target..."
    find "$TARGET_RES" -name "ic_launcher.webp" -delete
    find "$TARGET_RES" -name "ic_launcher.png" -delete
    find "$TARGET_RES" -name "ic_launcher_round.webp" -delete
    find "$TARGET_RES" -name "ic_launcher_round.png" -delete
    echo "📂 Copying correct icons from $SOURCE_ICONS..."
    cp -R "$SOURCE_ICONS/"* "$TARGET_RES/"
else
    echo "❌ Target resources or source icons not found. Build may fail."
fi

echo "Resuming build with Gradle..."
cd target/dx/scan2fil/release/android/app
./gradlew assembleRelease
cd -

echo
echo "✅ Build complete"
