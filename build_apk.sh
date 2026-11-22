#!/bin/bash
# Written by ShipScreen before each build — edits here are replaced.

# build_apk.sh - Build Android APK (Debug/Installable) for scan2fil
# Use this for local testing on a device/emulator

set -e

export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk"
NDK_VERSION=$(ls -1 "$ANDROID_NDK_HOME" 2>/dev/null | grep -E '^[0-9]+\.' | sort -V | tail -1)
export ANDROID_NDK_HOME="$ANDROID_NDK_HOME/$NDK_VERSION"
BUILD_TOOLS_HOME="$ANDROID_HOME/build-tools"
LATEST_BUILD_TOOLS=$(ls -1 "$BUILD_TOOLS_HOME" 2>/dev/null | sort -V | tail -1)
ANDROID_BUILD_TOOLS="$BUILD_TOOLS_HOME/$LATEST_BUILD_TOOLS"
ZIPALIGN="$ANDROID_BUILD_TOOLS/zipalign"
APKSIGNER="$ANDROID_BUILD_TOOLS/apksigner"

PROJECT_NAME="scan2fil"

echo "=== Building Android APK (Debug) for $PROJECT_NAME ==="
echo "NDK: $NDK_VERSION"
echo "Build Tools: $LATEST_BUILD_TOOLS"
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

echo "🧹 Cleaning previous build..."
rm -rf "target/dx/$PROJECT_NAME/release/android"

dx build --platform android --release

echo "🔧 Copying custom icons..."
RES_DIR="target/dx/$PROJECT_NAME/release/android/app/app/src/main/res"
find "$RES_DIR" -name "ic_launcher.webp" -delete
find "$RES_DIR" -name "ic_launcher_round.webp" -delete
# Remove adaptive icon XMLs — they override PNGs on Android 8+ and show the default robot icon
rm -f "$RES_DIR/mipmap-anydpi-v26/ic_launcher.xml"
rm -f "$RES_DIR/mipmap-anydpi-v26/ic_launcher_round.xml"
cp -r manual_assets/android_icons/mipmap-* "$RES_DIR/"
echo "✅ Icons copied successfully."

BUILD_DIR="target/dx/$PROJECT_NAME/release/android/app"
BUILD_GRADLE="$BUILD_DIR/app/build.gradle.kts"
OLD_PACKAGE="com.example.Scan2fil"
NEW_PACKAGE="com.mayorana.scan2fil"

if grep -q "$OLD_PACKAGE" "$BUILD_GRADLE"; then
    echo "⚠️  Incorrect package name detected ($OLD_PACKAGE)."
    echo "🔧 Applying automatic fix to set package to: $NEW_PACKAGE"
    sed -i '' "s/$OLD_PACKAGE/$NEW_PACKAGE/g" "$BUILD_GRADLE"
    find "$BUILD_DIR/app/src" -name "*.kt" -type f -exec sed -i '' "s/$OLD_PACKAGE/$NEW_PACKAGE/g" {} \;
    find "$BUILD_DIR/app/src" -name "AndroidManifest.xml" -type f -exec sed -i '' "s/$OLD_PACKAGE/$NEW_PACKAGE/g" {} \;
else
    echo "✅ Package name appears correct."
fi

echo "🔧 Ensuring App Name is 'scan2fil'..."
find "$BUILD_DIR/app/src" -name "AndroidManifest.xml" -type f -exec sed -i '' 's/android:label="@string\/app_name"/android:label="scan2fil"/g' {} \;

echo "↻ Rebuilding with Gradle..."
CURRENT_DIR=$(pwd)
cd "$BUILD_DIR"
./gradlew clean assembleDebug --configuration-cache
cd "$CURRENT_DIR"

echo
echo "✅ Gradle Build complete"

GENERATED_APK="$(find target/dx -name "*.apk" -type f -exec ls -t {} + 2>/dev/null | head -1)"
ALIGNED_APK="$(dirname "$GENERATED_APK")/app-aligned.apk"
SIGNED_APK="$(dirname "$GENERATED_APK")/scan2fil-signed.apk"

if [[ -f "$GENERATED_APK" ]]; then
    echo "🔧 Optimizing APK..."
    echo "  > Aligning..."
    rm -f "$ALIGNED_APK"
    "$ZIPALIGN" -v -p 4 "$GENERATED_APK" "$ALIGNED_APK" > /dev/null
    echo "  > Signing (v1 + v2)..."
    "$APKSIGNER" sign --ks "$HOME/.android/debug.keystore" \
                      --ks-pass pass:android \
                      --key-pass pass:android \
                      --out "$SIGNED_APK" \
                      "$ALIGNED_APK"
    echo "  > Verifying..."
    "$APKSIGNER" verify "$SIGNED_APK"
    rm "$ALIGNED_APK"
    echo "✅ Signed APK created: $SIGNED_APK"
    touch "$SIGNED_APK"
fi

if [[ -f "$SIGNED_APK" ]]; then
    TARGET_APK="./scan2fil-debug.apk"
    cp "$SIGNED_APK" "$TARGET_APK"
    echo "📦 APK copied to: $TARGET_APK"
    open -R "$TARGET_APK"
    open "$(pwd)"
else
    APK_PATH="$(find target/dx -name "*.apk" -type f -exec ls -t {} + 2>/dev/null | head -1)"
    if [[ -n "$APK_PATH" ]]; then
        TARGET_APK="./scan2fil-debug.apk"
        cp "$APK_PATH" "$TARGET_APK"
        echo "📦 APK copied to: $TARGET_APK"
        open -R "$TARGET_APK"
        open "$(pwd)"
    fi
fi
