#!/bin/bash

# DNP3 Tester - Windows Cross-Compilation Script
# Compiles the project to a single Windows .exe file with embedded frontend

set -e

echo "🚀 DNP3 Tester - Windows Cross-Compilation"
echo "========================================="
echo ""

# Check if cross is installed
if ! command -v cross &> /dev/null; then
    echo "📦 Installing 'cross' for cross-compilation..."
    cargo install cross --git https://github.com/cross-rs/cross
fi

# Windows targets
TARGETS=(
    "x86_64-pc-windows-gnu"
)

echo "🎯 Target: Windows x64"
echo ""

# Add Windows target
echo "📥 Adding Windows target..."
rustup target add x86_64-pc-windows-gnu

# Build for Windows
echo ""
echo "🔨 Building for Windows (x86_64)..."
cargo build --release --target x86_64-pc-windows-gnu

# Check if build succeeded
if [ -f "target/x86_64-pc-windows-gnu/release/dnp3_tester.exe" ]; then
    echo ""
    echo "✅ Build successful!"
    echo ""
    echo "📦 Output files:"
    echo "   Windows x64: target/x86_64-pc-windows-gnu/release/dnp3_tester.exe"
    
    # Get file size
    SIZE=$(du -h "target/x86_64-pc-windows-gnu/release/dnp3_tester.exe" | cut -f1)
    echo "   Size: $SIZE"
    
    echo ""
    echo "🎉 Done! You can now distribute the .exe file."
    echo ""
    echo "📋 Notes:"
    echo "   • The .exe includes all frontend files (HTML/CSS/JS)"
    echo "   • No additional files needed - it's a single executable"
    echo "   • Users just double-click dnp3_tester.exe to run"
else
    echo ""
    echo "❌ Build failed! Check errors above."
    exit 1
fi
