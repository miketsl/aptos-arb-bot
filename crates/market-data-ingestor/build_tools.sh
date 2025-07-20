#!/bin/bash

# Build script for MDI CLI tools
# This script builds all the CLI tools and sets up the development environment

set -e

echo "Building Market Data Ingestor CLI Tools"
echo "======================================"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    print_error "Please run this script from the market-data-ingestor crate directory"
    exit 1
fi

print_status "Checking Rust toolchain..."
if ! command -v cargo &> /dev/null; then
    print_error "Cargo not found. Please install Rust toolchain."
    exit 1
fi

print_status "Checking dependencies..."
cargo check --quiet
if [ $? -ne 0 ]; then
    print_error "Dependency check failed. Please fix cargo issues first."
    exit 1
fi

print_success "Dependencies OK"

# Build the CLI tools
print_status "Building CLI tools..."

echo ""
echo "Building mdi-recorder..."
cargo build --bin mdi-recorder --release
if [ $? -eq 0 ]; then
    print_success "mdi-recorder built successfully"
else
    print_error "Failed to build mdi-recorder"
    exit 1
fi

echo ""
echo "Building mdi-converter..."
cargo build --bin mdi-converter --release
if [ $? -eq 0 ]; then
    print_success "mdi-converter built successfully"
else
    print_error "Failed to build mdi-converter"
    exit 1
fi

echo ""
echo "Building mdi-tools..."
cargo build --bin mdi-tools --release
if [ $? -eq 0 ]; then
    print_success "mdi-tools built successfully"
else
    print_error "Failed to build mdi-tools"
    exit 1
fi

# Run tests
print_status "Running CLI tests..."
cargo test enhanced_cli_tests --release
if [ $? -eq 0 ]; then
    print_success "All tests passed"
else
    print_warning "Some tests failed, but build completed"
fi

# Create sample configuration if it doesn't exist
if [ ! -f "recording_config_sample.yml" ]; then
    print_status "Generating sample recording configuration..."
    ./target/release/mdi-recorder --generate-config
    if [ $? -eq 0 ]; then
        print_success "Sample configuration generated"
    else
        print_warning "Failed to generate sample configuration"
    fi
fi

# Create symlinks for easy access (optional)
print_status "Creating convenience symlinks..."
mkdir -p ../../target/tools

ln -sf "$(pwd)/target/release/mdi-recorder" ../../target/tools/mdi-recorder
ln -sf "$(pwd)/target/release/mdi-converter" ../../target/tools/mdi-converter  
ln -sf "$(pwd)/target/release/mdi-tools" ../../target/tools/mdi-tools

print_success "Symlinks created in ../../target/tools/"

# Display tool information
echo ""
echo "======================================"
print_success "CLI Tools Build Complete!"
echo "======================================"
echo ""
echo "Available tools:"
echo "  • mdi-recorder: Professional recording with pool state management"
echo "  • mdi-converter: Advanced conversion with batch processing"
echo "  • mdi-tools: Unified interface with analysis capabilities"
echo ""
echo "Tool locations:"
echo "  • Release binaries: ./target/release/"
echo "  • Convenience symlinks: ../../target/tools/"
echo ""
echo "Quick start:"
echo "  1. Generate sample config: ./target/release/mdi-recorder --generate-config"
echo "  2. Edit recording_config_sample.yml as needed"
echo "  3. Start recording: ./target/release/mdi-recorder --config-path ../../config/default.yml"
echo ""
echo "Documentation:"
echo "  • CLI_TOOLS_README.md: Comprehensive usage guide"
echo "  • recording_config_sample.yml: Configuration reference"
echo ""

# Check for common issues
print_status "Performing post-build checks..."

# Check if gzip is available for compression
if ! command -v gzip &> /dev/null; then
    print_warning "gzip not found. File compression features will be disabled."
fi

# Check available disk space
available_space=$(df . | tail -1 | awk '{print $4}')
if [ "$available_space" -lt 1048576 ]; then  # Less than 1GB
    print_warning "Low disk space detected. Consider enabling file rotation for recordings."
fi

# Verify tool functionality
print_status "Verifying tool functionality..."

echo "Testing mdi-recorder --help..."
if ./target/release/mdi-recorder --help > /dev/null 2>&1; then
    print_success "mdi-recorder help works"
else
    print_error "mdi-recorder help failed"
fi

echo "Testing mdi-converter --help..."
if ./target/release/mdi-converter --help > /dev/null 2>&1; then
    print_success "mdi-converter help works"
else
    print_error "mdi-converter help failed"
fi

echo "Testing mdi-tools --help..."
if ./target/release/mdi-tools --help > /dev/null 2>&1; then
    print_success "mdi-tools help works"
else
    print_error "mdi-tools help failed"
fi

echo ""
print_success "Build and verification complete!"
echo ""
echo "Next steps:"
echo "  1. Review CLI_TOOLS_README.md for detailed usage instructions"
echo "  2. Customize recording_config_sample.yml for your environment"
echo "  3. Test with a short recording session"
echo "  4. Set up monitoring and alerting as needed"
echo ""
echo "For support, refer to the documentation or check the test files for examples."