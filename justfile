default: install

# Build in debug mode
build:
    cargo build

# Build in release mode
release:
    cargo build --release

# Run the app
run:
    cargo run

# Install to ~/.local/bin
install: release
    cp target/release/muse ~/.local/bin/
    codesign -s - ~/.local/bin/muse

# Uninstall from /usr/local/bin
uninstall:
    rm -f /usr/local/bin/muse

# Remove build artifacts
clean:
    cargo clean

# Build Swift-only (legacy)
build-swift:
    swift build

# Run Swift version (legacy)
run-swift:
    swift run muse
