#!/bin/bash
set -e

# Configuration
LIB_NAME="ermis_call_node_binding"
PROJECT_DIR=$(pwd)
BUILD_DIR="$PROJECT_DIR/out/ios"
SWIFT_DIR="$BUILD_DIR/swift"
PACKAGE_NAME="ErmisFmp4Parser"

echo "🦀 Simple UniFFI Build Script"
echo "=============================="

# Clean previous builds
rm -rf "$BUILD_DIR"
mkdir -p "$SWIFT_DIR"

## Step 1: Install uniffi-bindgen if not already installed
#echo "📦 Checking for uniffi_bindgen..."
#if ! command -v uniffi_bindgen &> /dev/null; then
#    echo "Installing uniffi-bindgen..."
#    cargo install uniffi-bindgen --version 0.28.0
#else
#    echo "✓ uniffi_bindgen already installed"
#fi

# Step 2: Build for macOS (needed for bindgen)
echo "💻 Building for macOS..."
cargo build --release

# Step 3: Generate Swift bindings
echo "⚡ Generating Swift bindings..."

# Try multiple methods to generate bindings

# Method A: Using the built library directly
if [ -f "target/release/lib${LIB_NAME}.dylib" ]; then
    echo "Attempting with .dylib..."
    cargo run --bin uniffi-bindgen generate \
        --library target/release/lib${LIB_NAME}.dylib \
        --language swift \
        --out-dir "$SWIFT_DIR" 2>/dev/null || true
fi

# Method B: Using the static library
if [ ! -f "$SWIFT_DIR/${LIB_NAME}.swift" ] && [ -f "target/release/lib${LIB_NAME}.a" ]; then
    echo "Attempting with .a..."
    cargo run --bin uniffi-bindgen generate \
        --library target/release/lib${LIB_NAME}.a \
        --language swift \
        --out-dir "$SWIFT_DIR" 2>/dev/null || true
fi

# Method C: Using scaffolding from the library using cdylib
if [ ! -f "$SWIFT_DIR/${LIB_NAME}.swift" ]; then
    echo "Attempting with cargo build as cdylib..."
    # Ensure cdylib is built
    cargo build --release

    if [ -f "target/release/lib${LIB_NAME}.dylib" ]; then
        cargo run --bin uniffi-bindgen generate \
            --library target/release/lib${LIB_NAME}.dylib \
            --language swift \
            --out-dir "$SWIFT_DIR"
    fi
fi

# Verify generation succeeded
if [ ! -f "$SWIFT_DIR/${LIB_NAME}.swift" ]; then
    echo "❌ Failed to generate Swift bindings!"
    echo ""
    echo "Troubleshooting:"
    echo "1. Make sure Cargo.toml has: crate-type = [\"cdylib\", \"staticlib\"]"
    echo "2. Make sure you have uniffi = { version = \"0.28\" } in dependencies"
    echo "3. Check that uniffi::setup_scaffolding!() is called in lib.rs"
    echo ""
    echo "Manual generation:"
    echo "  cargo build --release"
    echo "  uniffi-bindgen generate --library target/release/lib${LIB_NAME}.dylib --language swift --out-dir build/swift"
    exit 1
fi

echo "✅ Swift bindings generated successfully!"

# Step 4: Build for iOS targets (optional, comment out if not needed)
echo ""
echo "📱 Building for iOS targets..."
echo "Installing iOS targets if needed..."

rustup target add aarch64-apple-ios 2>/dev/null || true
rustup target add aarch64-apple-ios-sim 2>/dev/null || true
rustup target add x86_64-apple-ios 2>/dev/null || true

echo "Building for iOS Device (arm64)..."
cargo build --release --target aarch64-apple-ios

echo "Building for iOS Simulator (arm64)..."
cargo build --release --target aarch64-apple-ios-sim

echo "Building for iOS Simulator (x86_64)..."
cargo build --release --target x86_64-apple-ios

# Step 5: Create universal binaries
echo ""
echo "🔨 Creating universal binaries..."

mkdir -p "$BUILD_DIR/ios-simulator"
lipo -create \
    target/x86_64-apple-ios/release/lib${LIB_NAME}.a \
    target/aarch64-apple-ios-sim/release/lib${LIB_NAME}.a \
    -output "$BUILD_DIR/ios-simulator/lib${LIB_NAME}.a"

mkdir -p "$BUILD_DIR/ios"
cp target/aarch64-apple-ios/release/lib${LIB_NAME}.a "$BUILD_DIR/ios/"

mkdir -p "$BUILD_DIR/macos"
cp target/release/lib${LIB_NAME}.a "$BUILD_DIR/macos/"

# Step 6: Create Swift Package structure
echo ""
echo "📦 Creating Swift Package structure..."

SPM_DIR="$BUILD_DIR/${PACKAGE_NAME}"
mkdir -p "$SPM_DIR/Sources/${PACKAGE_NAME}"
mkdir -p "$SPM_DIR/Sources/${PACKAGE_NAME}FFI/include"

# Copy Swift file
cp "$SWIFT_DIR/${LIB_NAME}.swift" "$SPM_DIR/Sources/${PACKAGE_NAME}/${PACKAGE_NAME}.swift"

# Copy C headers
cp "$SWIFT_DIR/${LIB_NAME}FFI.h" "$SPM_DIR/Sources/${PACKAGE_NAME}FFI/include/${PACKAGE_NAME}FFI.h" 2>/dev/null || true

# Create XCFramework for easier distribution
echo "🔨 Creating XCFramework..."

create_framework() {
    local PLATFORM=$1
    local LIB_PATH=$2
    local FRAMEWORK_DIR="$BUILD_DIR/frameworks/${PLATFORM}/${LIB_NAME}FFI.framework"
    
    mkdir -p "$FRAMEWORK_DIR/Headers"
    mkdir -p "$FRAMEWORK_DIR/Modules"
    
    # Copy library
    cp "$LIB_PATH" "$FRAMEWORK_DIR/${LIB_NAME}FFI"
    
    # Copy headers
    cp "$SWIFT_DIR/${LIB_NAME}FFI.h" "$FRAMEWORK_DIR/Headers/"
    
    # Create module map
    cat > "$FRAMEWORK_DIR/Modules/module.modulemap" <<EOF
framework module ${LIB_NAME}FFI {
    umbrella header "${LIB_NAME}FFI.h"
    export *
    module * { export * }
}
EOF
    
    # Create Info.plist
    cat > "$FRAMEWORK_DIR/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>${LIB_NAME}FFI</string>
    <key>CFBundleIdentifier</key>
    <string>network.ermis.callnode</string>
    <key>CFBundleName</key>
    <string>${LIB_NAME}FFI</string>
    <key>CFBundlePackageType</key>
    <string>FMWK</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
</dict>
</plist>
EOF
}

# Create frameworks for each platform
create_framework "ios" "$BUILD_DIR/ios/lib${LIB_NAME}.a"
create_framework "ios-simulator" "$BUILD_DIR/ios-simulator/lib${LIB_NAME}.a"
create_framework "macos" "$BUILD_DIR/macos/lib${LIB_NAME}.a"

# Build XCFramework
xcodebuild -create-xcframework \
    -framework "$BUILD_DIR/frameworks/ios/${LIB_NAME}FFI.framework" \
    -framework "$BUILD_DIR/frameworks/ios-simulator/${LIB_NAME}FFI.framework" \
    -framework "$BUILD_DIR/frameworks/macos/${LIB_NAME}FFI.framework" \
    -output "$SPM_DIR/${PACKAGE_NAME}FFI.xcframework"

# Generate Package.swift
echo "📝 Generating Package.swift..."

cat > "$SPM_DIR/Package.swift" <<EOF
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "${PACKAGE_NAME}",
    platforms: [
        .iOS(.v13),
        .macOS(.v10_15)
    ],
    products: [
        .library(
            name: "${PACKAGE_NAME}",
            targets: ["${PACKAGE_NAME}"]
        ),
    ],
    targets: [
        .target(
            name: "${PACKAGE_NAME}",
            dependencies: ["${PACKAGE_NAME}FFI"],
            path: "Sources/${PACKAGE_NAME}"
        ),
        .binaryTarget(
            name: "${PACKAGE_NAME}FFI",
            path: "${PACKAGE_NAME}FFI.xcframework"
        )
    ]
)
EOF

# Copy Swift file to main build directory for reference
cp "$SWIFT_DIR/${LIB_NAME}.swift" "$BUILD_DIR/${PACKAGE_NAME}.swift"
cp "$SWIFT_DIR/${LIB_NAME}FFI.h" "$BUILD_DIR/${PACKAGE_NAME}FFI.h" 2>/dev/null || true

echo ""
echo "✅ Build complete!"
echo "=============================="
echo "📁 Output directory: $BUILD_DIR"
echo "📄 Swift file: $BUILD_DIR/${LIB_NAME}.swift"
echo "📦 iOS library: $BUILD_DIR/ios/lib${LIB_NAME}.a"
echo "📦 iOS Simulator: $BUILD_DIR/ios-simulator/lib${LIB_NAME}.a"
echo "📦 macOS library: $BUILD_DIR/macos/lib${LIB_NAME}.a"
echo ""