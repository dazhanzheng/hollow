# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**hollow** is a macOS app that aims to be an AI-driven, semantics-centered personal file ingestion, understanding, archiving, and retrieval system. Instead of managing files by path, users manage files by meaning — files are automatically understood, tagged, summarized, and made searchable via natural language. See `WHITEPAPER.md` for the full product vision (written in Chinese).

The project is in its initial scaffolding stage.

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

## Architecture

The project follows a **local-first** architecture: most data and processing stays on the user's Mac, only AI tasks that require large models go to the cloud server.

### macOS Client (Swift/SwiftUI + hollow-core)

Everything the user interacts with runs locally:

- `hollow/` — Swift app source (entry point: `hollowApp.swift`, root view: `ContentView.swift`)
- `hollowTests/` — Unit tests using Swift Testing framework
- `hollowUITests/` — UI tests using XCTest
- `hollow-core/` — Rust library linked into the Swift app via FFI (C-compatible static library)

**Local responsibilities:**
- Intake: file folder monitoring (FSEvents), drag & drop UI, file metadata extraction, hash dedup
- Ingestion: PDF/Word/Excel/PPT text extraction, basic OCR (macOS Vision framework), archive decompression
- Storage & Index: SQLite metadata DB, full-text index, vector index (ANN search), operation logs, undo/rollback
- Search: local metadata + full-text + vector retrieval, rule-based archive strategy
- UI: search box, file cards, timeline, relationship view, settings, archive confirmation

### Cloud Server (hollow-server)

A lightweight API proxy deployed on a cloud server. Only handles tasks that require LLM/GPU and cannot run on a MacBook:

- `hollow-server/` — Rust binary, independent of hollow-core

**Cloud responsibilities:**
- LLM API proxy: summarization, classification, tagging, naming suggestions, sensitivity detection, relationship inference
- Embedding API proxy: vector generation (results returned to client for local storage)
- Query understanding: natural language → search plan
- Result re-ranking: semantic re-ranking of candidate results
- Heavy AI: high-accuracy OCR, audio/video transcription
- Future: user accounts, multi-device sync
