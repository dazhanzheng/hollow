# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**hollow** is a macOS app that aims to be an AI-driven, semantics-centered personal file ingestion, understanding, archiving, and retrieval system. Instead of managing files by path, users manage files by meaning — files are automatically understood, tagged, summarized, and made searchable via natural language. See `WHITEPAPER.md` for the full product vision (written in Chinese).

The project is in its initial scaffolding stage.

## Documentation

For detailed project documentation (product vision, architecture, data model, module design), see [docs/INDEX.md](docs/INDEX.md). Start there and follow links to the specific document you need.

## Build & Run

### macOS Client (Swift / Xcode)

This is an Xcode project. All build configuration lives in `hollow.xcodeproj`.

```bash
# Build
xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build

# Run tests (unit)
xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' test

# Run a single test (by class or method)
xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' \
  -only-testing:hollowTests/hollowTests/example test
```

### Rust (Core + Server)

Rust code is organized as a Cargo workspace at the repo root with two crates.

```bash
# Build all Rust crates
cargo build

# Run all Rust tests
cargo test

# Build/test a single crate
cargo build -p hollow-core
cargo test -p hollow-server

# Run the server
RUST_LOG=debug cargo run -p hollow-server
```

## Tech Stack

- **Platform**: macOS 26.2+
- **Client**: Swift 6, SwiftUI (Xcode 26)
- **Local Core**: Rust library (`hollow-core/`) — linked into Swift app via FFI
- **Cloud Server**: Rust binary (`hollow-server/`) — lightweight API proxy for LLM/Embedding services
- **Testing**: Swift Testing (`import Testing`) for unit tests, XCTest for UI tests; `cargo test` for Rust
- **Bundle ID**: `com.syncpulse.hollow`

