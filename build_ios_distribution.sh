#!/bin/bash
# Written by ShipScreen before each build — edits here are replaced.

# build_ios_distribution.sh - Consolidated Build Script for scan2fil
# Handles building, validation fixes, signing (with Entitlements), and packaging.

set -e

APP_NAME="scan2fil"
# Bundle Identifier
BUNDLE_ID="com.mayorana.scan2fil"

# Signing Identity (Distribution)
IDENTITY="Apple Distribution: Mohamed Bennekrouf (CHK7MBH3BK)"

# Paths
OUTPUT_DIR="target/ios/ipa"
DX_IOS_DIR="target/dx/scan2fil/release/ios"
ENTITLEMENTS="Entitlements.plist"

echo "🚀 Starting iOS Build for App Store Distribution..."

# 1. Prerequisite Checks
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

# 1.5 Normalize icon to genuine PNG format
# Apple's validator reads the actual file format, not just the extension.
# A JPEG renamed to .png will pass the filename check but fail validation.
if [ -f "assets/icon.png" ]; then
  sips -s format png "assets/icon.png" --out "assets/icon.png" >/dev/null 2>&1
  echo "✅ Icon normalized to true PNG format."
fi

# 2. Build Rust Project for iOS (Release)
echo "🧹 Cleaning old builds..."
rm -rf target/dx/scan2fil/release/ios
rm -rf target/dx/scan2fil/release/web # Clean web assets cache too

echo "📦 Building Rust project for iOS (Release - Device)..."
# Force aarch64-apple-ios to avoid simulator slices
dx build --platform ios --release --target aarch64-apple-ios

# 3. Locate Generated App Bundle (must be after dx build)
APP_BUNDLE=$(find "$DX_IOS_DIR" -maxdepth 1 -name "*.app" -type d 2>/dev/null | head -1)
if [ -z "$APP_BUNDLE" ] || [ ! -d "$APP_BUNDLE" ]; then
  echo "❌ Could not find generated .app bundle in $DX_IOS_DIR"
  ls "$DX_IOS_DIR" 2>/dev/null || echo "   (directory does not exist)"
  exit 1
fi
echo "✅ Found App Bundle: $APP_BUNDLE"

# 4. Prepare Payload Directory
echo "📦 Packaging IPA structure..."
mkdir -p "$OUTPUT_DIR"
rm -rf "$OUTPUT_DIR/Payload"
mkdir -p "$OUTPUT_DIR/Payload"

# Copy App Bundle to Payload
cp -R "$APP_BUNDLE" "$OUTPUT_DIR/Payload/"

# Define paths for the copied app
APP_PATH="$OUTPUT_DIR/Payload/$(basename "$APP_BUNDLE")"
PLIST_PATH="$APP_PATH/Info.plist"

# 4.5 Generate App Icons via actool → Assets.car + correct plist
# The root cause of "Missing icon" validation failures was that actool requires
# the output directory to exist before running, and was silently failing.
echo "🎨 Generating App Icons..."
ICON_SOURCE="assets/icon.png"

if [ -f "$ICON_SOURCE" ]; then
  echo "   Found source icon: $ICON_SOURCE"

  # Convert to true PNG and strip alpha (Apple rejects transparency and JPEG-as-PNG)
  FLAT_ICON="/tmp/icon_flat_$$.png"
  sips -s format png "$ICON_SOURCE" --out "$FLAT_ICON" >/dev/null 2>&1
  echo "   Converted to true PNG."

  # Build asset catalog in temp dir
  CATALOG_DIR="/tmp/shipscreen_icons_$$"
  ICON_SET="$CATALOG_DIR/Assets.xcassets/AppIcon.appiconset"
  mkdir -p "$ICON_SET"

  # iPhone sizes
  sips -z 40   40   "$FLAT_ICON" --out "$ICON_SET/Icon-20@2x.png"      >/dev/null
  sips -z 60   60   "$FLAT_ICON" --out "$ICON_SET/Icon-20@3x.png"      >/dev/null
  sips -z 58   58   "$FLAT_ICON" --out "$ICON_SET/Icon-29@2x.png"      >/dev/null
  sips -z 87   87   "$FLAT_ICON" --out "$ICON_SET/Icon-29@3x.png"      >/dev/null
  sips -z 80   80   "$FLAT_ICON" --out "$ICON_SET/Icon-40@2x.png"      >/dev/null
  sips -z 120  120  "$FLAT_ICON" --out "$ICON_SET/Icon-40@3x.png"      >/dev/null
  sips -z 120  120  "$FLAT_ICON" --out "$ICON_SET/Icon-60@2x.png"      >/dev/null
  sips -z 180  180  "$FLAT_ICON" --out "$ICON_SET/Icon-60@3x.png"      >/dev/null
  # iPad sizes
  sips -z 20   20   "$FLAT_ICON" --out "$ICON_SET/Icon-ipad-20@1x.png" >/dev/null
  sips -z 40   40   "$FLAT_ICON" --out "$ICON_SET/Icon-ipad-20@2x.png" >/dev/null
  sips -z 29   29   "$FLAT_ICON" --out "$ICON_SET/Icon-ipad-29@1x.png" >/dev/null
  sips -z 58   58   "$FLAT_ICON" --out "$ICON_SET/Icon-ipad-29@2x.png" >/dev/null
  sips -z 40   40   "$FLAT_ICON" --out "$ICON_SET/Icon-ipad-40@1x.png" >/dev/null
  sips -z 80   80   "$FLAT_ICON" --out "$ICON_SET/Icon-ipad-40@2x.png" >/dev/null
  sips -z 76   76   "$FLAT_ICON" --out "$ICON_SET/Icon-ipad-76@1x.png" >/dev/null
  sips -z 152  152  "$FLAT_ICON" --out "$ICON_SET/Icon-ipad-76@2x.png" >/dev/null
  sips -z 167  167  "$FLAT_ICON" --out "$ICON_SET/Icon-ipad-83@2x.png" >/dev/null
  # App Store
  sips -z 1024 1024 "$FLAT_ICON" --out "$ICON_SET/Icon-1024.png"       >/dev/null
  rm -f "$FLAT_ICON"

  cat > "$ICON_SET/Contents.json" << 'ICONEOF'
{
  "images": [
    {"idiom":"iphone","scale":"2x","size":"20x20","filename":"Icon-20@2x.png"},
    {"idiom":"iphone","scale":"3x","size":"20x20","filename":"Icon-20@3x.png"},
    {"idiom":"iphone","scale":"2x","size":"29x29","filename":"Icon-29@2x.png"},
    {"idiom":"iphone","scale":"3x","size":"29x29","filename":"Icon-29@3x.png"},
    {"idiom":"iphone","scale":"2x","size":"40x40","filename":"Icon-40@2x.png"},
    {"idiom":"iphone","scale":"3x","size":"40x40","filename":"Icon-40@3x.png"},
    {"idiom":"iphone","scale":"2x","size":"60x60","filename":"Icon-60@2x.png"},
    {"idiom":"iphone","scale":"3x","size":"60x60","filename":"Icon-60@3x.png"},
    {"idiom":"ipad","scale":"1x","size":"20x20","filename":"Icon-ipad-20@1x.png"},
    {"idiom":"ipad","scale":"2x","size":"20x20","filename":"Icon-ipad-20@2x.png"},
    {"idiom":"ipad","scale":"1x","size":"29x29","filename":"Icon-ipad-29@1x.png"},
    {"idiom":"ipad","scale":"2x","size":"29x29","filename":"Icon-ipad-29@2x.png"},
    {"idiom":"ipad","scale":"1x","size":"40x40","filename":"Icon-ipad-40@1x.png"},
    {"idiom":"ipad","scale":"2x","size":"40x40","filename":"Icon-ipad-40@2x.png"},
    {"idiom":"ipad","scale":"1x","size":"76x76","filename":"Icon-ipad-76@1x.png"},
    {"idiom":"ipad","scale":"2x","size":"76x76","filename":"Icon-ipad-76@2x.png"},
    {"idiom":"ipad","scale":"2x","size":"83.5x83.5","filename":"Icon-ipad-83@2x.png"},
    {"idiom":"ios-marketing","scale":"1x","size":"1024x1024","filename":"Icon-1024.png"}
  ],
  "info": {"author":"xcode","version":1}
}
ICONEOF

  # APP_PATH already exists (bundle was copied there). actool writes Assets.car into it.
  PARTIAL_PLIST="/tmp/partial_info_$$.plist"
  echo "   Compiling asset catalog with actool..."
  xcrun actool "$CATALOG_DIR/Assets.xcassets" \
    --compile "$APP_PATH" \
    --platform iphoneos \
    --minimum-deployment-target 14.0 \
    --app-icon AppIcon \
    --output-partial-info-plist "$PARTIAL_PLIST" 2>&1 | grep -v "^$" || true

  if [ -f "$PARTIAL_PLIST" ]; then
    # Remove existing icon keys then merge actool-generated ones
    /usr/libexec/PlistBuddy -c "Delete :CFBundleIcons" "$PLIST_PATH" 2>/dev/null || true
    /usr/libexec/PlistBuddy -c "Delete :CFBundleIcons~ipad" "$PLIST_PATH" 2>/dev/null || true
    /usr/libexec/PlistBuddy -c "Delete :CFBundleIconName" "$PLIST_PATH" 2>/dev/null || true
    /usr/libexec/PlistBuddy -c "Merge $PARTIAL_PLIST" "$PLIST_PATH"
    rm -f "$PARTIAL_PLIST"
    echo "✅ Assets.car generated and Info.plist updated."
  else
    echo "⚠️  actool did not produce a partial plist."
  fi

  rm -rf "$CATALOG_DIR"
else
  echo "⚠️  Warning: assets/icon.png not found. Skipping icon generation."
fi

echo "🔧 Fixing Info.plist for App Store Validation..."

# Remove empty lines at start of file
sed -i '' '/^$/d' "$PLIST_PATH"

# Inject Bundle Metadata
plutil -replace CFBundleIdentifier -string "$BUNDLE_ID" "$PLIST_PATH"
plutil -replace CFBundleDisplayName -string "scan2fil" "$PLIST_PATH"

# Build Number — owned by ShipScreen (Version step), injected per build
BUILD_NUMBER="4"
echo "   Version 1.0 ($BUILD_NUMBER)"

plutil -replace CFBundleVersion -string "$BUILD_NUMBER" "$PLIST_PATH"
plutil -replace CFBundleShortVersionString -string "1.0" "$PLIST_PATH"
plutil -replace MinimumOSVersion -string "14.0" "$PLIST_PATH"
plutil -replace CFBundlePackageType -string "APPL" "$PLIST_PATH"

# Enforce Single Supported Platform (Fixes Validation Error 90562)
plutil -replace CFBundleSupportedPlatforms -xml "<array><string>iPhoneOS</string></array>" "$PLIST_PATH"

# Encryption Export Compliance (Fixes Missing Compliance Warning)
plutil -replace ITSAppUsesNonExemptEncryption -bool NO "$PLIST_PATH"

# Inject UILaunchScreen (Requires iOS 14.0+)
plutil -remove UILaunchStoryboardName "$PLIST_PATH" || true
plutil -replace UILaunchScreen -xml "<dict>
        <key>UIColorName</key>
        <string>LaunchBackgroundColor</string>
        <key>UIImageName</key>
        <string>LaunchImage</string>
    </dict>" "$PLIST_PATH"

# Dynamic SDK/Platform Metadata
SDK_VERSION=$(xcrun --sdk iphoneos --show-sdk-version)
SDK_BUILD=$(xcrun --sdk iphoneos --show-sdk-build-version)

XCODE_VERSION_RAW=$(xcodebuild -version | grep "Xcode" | awk '{print $2}')
XCODE_BUILD=$(xcodebuild -version | grep "Build version" | awk '{print $3}')

MAJOR=$(echo "$XCODE_VERSION_RAW" | cut -d. -f1)
MINOR=$(echo "$XCODE_VERSION_RAW" | cut -d. -f2)
DT_XCODE="${MAJOR}${MINOR}0"

echo "   Detected Xcode: $XCODE_VERSION_RAW ($XCODE_BUILD) -> DTXcode: $DT_XCODE"
echo "   Detected SDK: iOS $SDK_VERSION ($SDK_BUILD)"

plutil -replace DTPlatformName -string "iphoneos" "$PLIST_PATH"
plutil -replace DTPlatformVersion -string "$SDK_VERSION" "$PLIST_PATH"
plutil -replace DTSDKName -string "iphoneos$SDK_VERSION" "$PLIST_PATH"
plutil -replace DTSDKBuild -string "$SDK_BUILD" "$PLIST_PATH"
plutil -replace DTPlatformBuild -string "$SDK_BUILD" "$PLIST_PATH"

plutil -replace DTXcode -string "$DT_XCODE" "$PLIST_PATH"
plutil -replace DTXcodeBuild -string "$XCODE_BUILD" "$PLIST_PATH"
plutil -replace DTCompiler -string "com.apple.compilers.llvm.clang.1_0" "$PLIST_PATH"

# 6. Embed Provisioning Profile
# Priority order:
#   1. ~/Downloads/<AppName>.mobileprovision  (project-specific, freshly downloaded)
#   2. ShipScreen global profile selection
#   3. embedded.mobileprovision in project root
#   4. Any .mobileprovision in project root
GLOBAL_PROFILE="/Users/mb/Library/MobileDevice/Provisioning Profiles/scan2fil-2026.mobileprovision"
NAMED_PROFILE=$(ls "$HOME/Downloads/scan2fil.mobileprovision" 2>/dev/null | head -1)

if [ -f "$NAMED_PROFILE" ]; then
  echo "📄 Using project profile from Downloads: $NAMED_PROFILE"
  cp "$NAMED_PROFILE" "$APP_PATH/embedded.mobileprovision"
elif [ -f "$GLOBAL_PROFILE" ]; then
  echo "📄 Using global profile: $GLOBAL_PROFILE"
  cp "$GLOBAL_PROFILE" "$APP_PATH/embedded.mobileprovision"
elif [ -f "embedded.mobileprovision" ]; then
  echo "📄 Found embedded.mobileprovision in root"
  cp "embedded.mobileprovision" "$APP_PATH/embedded.mobileprovision"
else
  PROVISION_PROFILE=$(find . -maxdepth 1 -name "*.mobileprovision" | head -n 1)
  if [ -f "$PROVISION_PROFILE" ]; then
    echo "📄 Found profile: $PROVISION_PROFILE"
    cp "$PROVISION_PROFILE" "$APP_PATH/embedded.mobileprovision"
  else
    echo "❌  ERROR: No .mobileprovision file found. Signing will fail!"
    exit 1
  fi
fi

# 6.5 Remove stale/duplicate assets
echo "🧹 Removing stale assets from app bundle..."
CSS_COUNT=$(find "$APP_PATH/assets" -name "*.css" 2>/dev/null | wc -l | tr -d ' ')
if [ "$CSS_COUNT" -gt 1 ]; then
  echo "   ⚠️  Found $CSS_COUNT CSS files - keeping only the newest"
  find "$APP_PATH/assets" -name "*.css" -print0 | xargs -0 ls -t | tail -n +2 | xargs rm -f
  echo "   ✅ Cleaned stale CSS files"
fi

# 7. Codesign with Entitlements
echo "✍️  Signing with identity: $IDENTITY"
echo "   Entitlements: $ENTITLEMENTS"

if [ ! -f "$ENTITLEMENTS" ]; then
  echo "❌ Entitlements file not found at $ENTITLEMENTS"
  exit 1
fi

# Remove existing signature
rm -rf "$APP_PATH/_CodeSignature"

# Codesign
codesign --force --deep --sign "$IDENTITY" --entitlements "$ENTITLEMENTS" --timestamp "$APP_PATH"

# Verify signature
echo "🔍 Verifying signature..."
codesign --verify --deep --strict --verbose=4 "$APP_PATH" 2>&1 || {
  echo "❌ Signature verification failed"
  exit 1
}

# 8. Create IPA
echo "📦 Creating .ipa..."
rm -f "$OUTPUT_DIR/$APP_NAME.ipa"
rm -f "./$APP_NAME.ipa"
pushd "$OUTPUT_DIR" >/dev/null
zip -r "$APP_NAME.ipa" Payload >/dev/null
popd >/dev/null

rm -rf "$OUTPUT_DIR/Payload"

echo "✅ Build Complete!"
echo "📂 IPA location: $OUTPUT_DIR/$APP_NAME.ipa"

cp "$OUTPUT_DIR/$APP_NAME.ipa" "./$APP_NAME.ipa"
echo "✅ IPA copied to project root: ./$APP_NAME.ipa"
open -R "./$APP_NAME.ipa"
