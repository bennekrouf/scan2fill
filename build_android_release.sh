#!/bin/bash
# Written by ShipScreen before each build — edits here are replaced.

# build_android_release.sh - Build Signed Android App Bundle (AAB) for Google Play
set -e

PROJECT_NAME="scan2fil"

# Signing — ShipScreen passes these in from Accounts → Android upload key
# (password from the Keychain); run by hand, set them in the project's .env:
#   ANDROID_KEYSTORE_PATH       upload keystore (.jks / .keystore)
#   ANDROID_KEYSTORE_PASSWORD   its password
#   ANDROID_KEY_ALIAS           optional, defaults to the project slug
#   ANDROID_KEY_PASSWORD        optional, defaults to the keystore password
KEYSTORE_PATH="${ANDROID_KEYSTORE_PATH:-}"
KEYSTORE_PATH="${KEYSTORE_PATH/#\~/$HOME}"
KEY_ALIAS="${ANDROID_KEY_ALIAS:-scan2fil}"
KEY_PASS="${ANDROID_KEYSTORE_PASSWORD:-}"
KEY_PASSWORD="${ANDROID_KEY_PASSWORD:-$KEY_PASS}"

export ANDROID_HOME="$HOME/Library/Android/sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk"
NDK_VERSION=$(ls -1 "$ANDROID_NDK_HOME" 2>/dev/null | grep -E '^[0-9]+\.' | sort -V | tail -1)
export ANDROID_NDK_HOME="$ANDROID_NDK_HOME/$NDK_VERSION"

echo "🚀 Starting Android Release Build (AAB)..."
echo "NDK: $NDK_VERSION"

# 0. Check tools
if [ -z "$NDK_VERSION" ]; then
    echo "❌ Android NDK not found in $ANDROID_HOME/ndk"
    echo "   Install it in Android Studio: SDK Manager → SDK Tools → NDK (Side by side)."
    exit 1
fi
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

# 0b. Check signing
if [ -z "$KEYSTORE_PATH" ] || [ -z "$KEY_PASS" ]; then
    echo "❌ Android signing is not set up."
    echo "   In ShipScreen: Accounts → Android upload key (the password stays in the Keychain)."
    echo "   Running this script by hand? Add to the project's .env:"
    echo "   ANDROID_KEYSTORE_PATH=/path/to/upload-keystore.jks"
    echo "   ANDROID_KEYSTORE_PASSWORD=<its password>"
    exit 1
fi
if [ ! -f "$KEYSTORE_PATH" ]; then
    echo "❌ Keystore not found: $KEYSTORE_PATH"
    echo "   Create an upload key with:"
    echo "   keytool -genkeypair -v -keystore \"$KEYSTORE_PATH\" -alias $KEY_ALIAS -keyalg RSA -keysize 2048 -validity 10000"
    exit 1
fi

# 1. Start Build
echo "🧹 Cleaning previous release build..."
rm -rf "target/dx/$PROJECT_NAME/release/android"

echo "📦 Running dx bundle..."
# dx's own final Gradle step can fail (duplicate icons); that is patched and
# rebuilt below. What must exist is the Gradle project dx generates first.
dx bundle --platform android --release || true
if [ ! -f "target/dx/$PROJECT_NAME/release/android/app/app/build.gradle.kts" ]; then
    echo "❌ dx bundle stopped before generating the Android project — see its error above."
    exit 1
fi

# 2. Resource & Icon Fixes
echo "🎨 Fixing resources and icons..."
RES_DIR="target/dx/$PROJECT_NAME/release/android/app/app/src/main/res"
SOURCE_ICONS="manual_assets/android_icons"

if [ -d "$RES_DIR" ] && [ -d "$SOURCE_ICONS" ]; then
    find "$RES_DIR" -name "ic_launcher.webp" -delete
    find "$RES_DIR" -name "ic_launcher_round.webp" -delete
    # Remove adaptive icon XMLs — they override PNGs on Android 8+ and show the default robot icon
    rm -f "$RES_DIR/mipmap-anydpi-v26/ic_launcher.xml"
    rm -f "$RES_DIR/mipmap-anydpi-v26/ic_launcher_round.xml"
    cp -r "$SOURCE_ICONS/mipmap-"* "$RES_DIR/"
    echo "✅ Icons updated."
else
    echo "⚠️  Warning: Resources directory not found."
fi

# 3. Package Name Fixes
echo "🔧 Fixing Package Name..."
BUILD_DIR="target/dx/$PROJECT_NAME/release/android/app"
BUILD_GRADLE="$BUILD_DIR/app/build.gradle.kts"
OLD_PACKAGE="com.example.Scan2fil"
NEW_PACKAGE="com.mayorana.scan2fil"

# Version info — owned by ShipScreen (Version step), injected per build.
# Mirrored into Dioxus.toml so the project's own config stays in step.
V_CODE=9
V_NAME="1.0"
sed -i '' "s/version_code *= *[0-9]*/version_code = $V_CODE/g" Dioxus.toml
sed -i '' "s/version_name *= *\".*\"/version_name = \"$V_NAME\"/g" Dioxus.toml

echo "   Package: $NEW_PACKAGE"
echo "   Version: $V_NAME ($V_CODE)"

if grep -q "$OLD_PACKAGE" "$BUILD_GRADLE" || grep -q "versionCode" "$BUILD_GRADLE"; then
    echo "   Updating build.gradle.kts..."
    # Fix package name
    sed -i '' "s/$OLD_PACKAGE/$NEW_PACKAGE/g" "$BUILD_GRADLE"
    
    # Fix Version Code
    sed -i '' "s/versionCode *= *[0-9]*/versionCode = $V_CODE/g" "$BUILD_GRADLE"
    
    # Fix Version Name
    sed -i '' "s/versionName *= *\".*\"/versionName = \"$V_NAME\"/g" "$BUILD_GRADLE"

    find "$BUILD_DIR/app/src" -name "*.kt" -type f -exec sed -i '' "s/$OLD_PACKAGE/$NEW_PACKAGE/g" {} \;
    find "$BUILD_DIR/app/src" -name "AndroidManifest.xml" -type f -exec sed -i '' "s/$OLD_PACKAGE/$NEW_PACKAGE/g" {} \;
    find "$BUILD_DIR/app/src" -name "AndroidManifest.xml" -type f -exec sed -i '' "s/android:label=\\\"@string\\/app_name\\\"/android:label=\\\"scan2fil\\\"/g" {} \;
fi

# 4. Inject Signing Config
echo "✍️  Injecting Signing Configuration..."

if ! grep -q "signingConfigs" "$BUILD_GRADLE"; then
    ABS_KEYSTORE_PATH="$KEYSTORE_PATH"

    if [[ "$(cat "$BUILD_GRADLE")" == *"buildTypes"* ]]; then
        echo "   Patching existing buildTypes..."
        sed -i '' "/buildTypes {/i \\
        signingConfigs {\\
            create(\"release\") {\\
                storeFile = file(\"$ABS_KEYSTORE_PATH\")\\
                storePassword = \"$KEY_PASS\"\\
                keyAlias = \"$KEY_ALIAS\"\\
                keyPassword = \"$KEY_PASSWORD\"\\
            }\\
        }\\
" "$BUILD_GRADLE"

        sed -i '' "/getByName(\"release\") {/a \\
            signingConfig = signingConfigs.getByName(\"release\")
" "$BUILD_GRADLE"
    else
        echo "⚠️  Could not find buildTypes block."
    fi
else
    echo "ℹ️  Signing config appears already present."
fi

# 5. Build AAB with Gradle
echo "🏗️  Building Android App Bundle..."
cd "$BUILD_DIR"
./gradlew bundleRelease

# 6. Verify and Move
# Standard Gradle output for signed release is 'app-release.aab'
# Do NOT use 'find' because it picks up the unsigned architecture-specific bundles instead of the final signed universal bundle.
OUTPUT_AAB="app/build/outputs/bundle/release/app-release.aab"
if [ -f "$OUTPUT_AAB" ]; then
    echo "✅ AAB Generated: $OUTPUT_AAB"
    cd - > /dev/null
    TARGET_NAME="${PROJECT_NAME}_release.aab"
    cp "$BUILD_DIR/$OUTPUT_AAB" "./$TARGET_NAME"
    echo "📦 Final Bundle copied to: ./$TARGET_NAME"

    echo "🔍 Verifying Signature..."
    if jarsigner -verify -verbose -certs "$TARGET_NAME" | grep -q "jar verified"; then
        echo "✅ Signature Verified!"
        echo "🎉 Ready for Google Play Console upload."
        open -R "./$TARGET_NAME"
        open "$(pwd)"
    else
        echo "❌ Signature Verification FAILED."
        exit 1
    fi
else
    echo "❌ Build Failed: AAB not found in app/build/outputs/bundle/release/"
    ls "$BUILD_DIR/app/build/outputs/bundle/release/" 2>/dev/null || echo "(directory does not exist)"
    exit 1
fi
