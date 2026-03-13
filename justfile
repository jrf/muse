default: install

# Build in debug mode
build:
    swift build

# Build in release mode
release:
    swift build -c release

# Run the app
run:
    swift run muse

# Install to ~/.local/bin
install: release
    cp .build/release/muse ~/.local/bin/
    codesign -s - ~/.local/bin/muse

# Uninstall from /usr/local/bin
uninstall:
    rm -f /usr/local/bin/muse

# Remove build artifacts
clean:
    swift package clean
