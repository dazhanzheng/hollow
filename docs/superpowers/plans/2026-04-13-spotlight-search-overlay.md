# Spotlight-Style Global Search Overlay — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a ⌥Space-triggered, NSPanel-based Spotlight-style floating search overlay that wraps the existing `hybrid_search` FFI, with 250ms debounced live search and a customizable hotkey in Settings.

**Architecture:** A SwiftUI view inside an `NSPanel` subclass managed by an `@Observable` `SpotlightCoordinator` singleton. Global hotkey via the `sindresorhus/KeyboardShortcuts` SPM package. Zero Rust changes — reuses `HollowBridge.shared.hybridSearch`.

**Tech Stack:** Swift 6, SwiftUI, AppKit (`NSPanel`, `NSVisualEffectView`, `NSHostingView`), Swift Testing (`import Testing`), `sindresorhus/KeyboardShortcuts` (new SPM dep).

**Spec:** [docs/superpowers/specs/2026-04-13-spotlight-search-overlay-design.md](../specs/2026-04-13-spotlight-search-overlay-design.md)

---

## Deviations from spec

Two simplifications discovered during plan writing:

1. **No relative timestamp on result rows.** Spec's Section "视觉规格" described a right-side "2d / 5h / 1w" label. The existing [SearchResult](../../../hollow/Generated/hollow_core.swift) FFI struct carries `fileId / fileName / currentPath / snippet / similarity / sources` — no timestamp. Adding one requires a Rust-side change and a new FTS5 join, which is out of scope. **v1 drops the right-side timestamp**; rows become `[icon, filename+snippet].`
2. **Empty state is a placeholder, not recent files.** Spec chose option B ("empty state shows recent files"). But [IngestionService.recentFiles](../../../hollow/IngestionService.swift) is `[String]` — filenames only, no paths or file IDs, so clicking a recent file couldn't actually open it. Rather than smuggle in a model change, **v1 shows a single centered placeholder row "Start typing to search..."** when `query` is empty.

Both are non-disruptive — they simplify the first release and leave room for later enhancement (adding a timestamp column + a recent-files data model are both additive changes).

---

## File Structure

**New files (under `hollow/Spotlight/`):**

| File | Responsibility |
|---|---|
| `KeyboardShortcutsNames.swift` | Extension declaring `KeyboardShortcuts.Name.spotlightSearch` with default `⌥Space` |
| `SpotlightCoordinator.swift` | `@Observable` `@MainActor` singleton. Owns `isVisible`, `query`, `results`, `selectedIndex`. Handles toggle/show/hide + debounced search + keyboard index movement. Panel lifecycle is delegated to the coordinator's `panel` property (lazily created). |
| `SpotlightPanel.swift` | `NSPanel` subclass with `canBecomeKey = true`, `.hudWindow` style mask, non-activating, floating level. Hosts `SpotlightView` via `NSHostingView`. |
| `SpotlightView.swift` | SwiftUI root view: `TextField` bound to coordinator, results list, keyboard shortcut bindings (↑↓, ↵, ⌘↵, ESC). |
| `SpotlightResultRow.swift` | One result row: icon + filename + FTS5 snippet (highlighted). Selected / unselected visuals. |

**New test file:**

| File | Responsibility |
|---|---|
| `hollowTests/SpotlightCoordinatorTests.swift` | Unit tests for the pure state-machine surface of the coordinator (toggle, onQueryChange, selected-index bounds, hide-clears-state). Uses an injected searcher closure to avoid calling real `hybrid_search`. |

**Modified files:**

| File | Change |
|---|---|
| `hollow.xcodeproj/project.pbxproj` | Add SPM dependency on `sindresorhus/KeyboardShortcuts` |
| [hollow/hollowApp.swift](../../../hollow/hollowApp.swift) | Instantiate `SpotlightCoordinator` in `init()`, register hotkey handler |
| [hollow/SettingsView.swift](../../../hollow/SettingsView.swift) | Add "Global Search" section in General tab with `KeyboardShortcuts.Recorder` |

**Unchanged:**
- Rust `hollow-core` — zero changes
- [hollow/MenuBarView.swift](../../../hollow/MenuBarView.swift), [hollow/SearchView.swift](../../../hollow/SearchView.swift), [hollow/HollowBridge.swift](../../../hollow/HollowBridge.swift)

---

## Task 1: Add `KeyboardShortcuts` SPM dependency

**Files:**
- Modify: `hollow.xcodeproj/project.pbxproj` (via Xcode GUI — pbxproj editing by hand is brittle)

- [ ] **Step 1: Add the package in Xcode**

Open `hollow.xcodeproj` in Xcode, then:
1. File → Add Package Dependencies…
2. Paste URL: `https://github.com/sindresorhus/KeyboardShortcuts`
3. Dependency Rule: "Up to Next Major Version" — 2.0.0
4. Add to target: `hollow` (the app target, not the test target)
5. Click "Add Package"

- [ ] **Step 2: Verify the dependency is resolved**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build 2>&1 | tail -30`
Expected: Build succeeds. `KeyboardShortcuts` appears in the "Build Phases → Link Binary With Libraries" list.

- [ ] **Step 3: Commit**

```bash
git add hollow.xcodeproj/project.pbxproj hollow.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved
git commit -m "deps: add sindresorhus/KeyboardShortcuts SPM package"
```

---

## Task 2: Declare the hotkey name with default

**Files:**
- Create: `hollow/Spotlight/KeyboardShortcutsNames.swift`

- [ ] **Step 1: Create the directory**

```bash
mkdir -p hollow/Spotlight
```

- [ ] **Step 2: Write the extension**

```swift
// hollow/Spotlight/KeyboardShortcutsNames.swift
import KeyboardShortcuts

extension KeyboardShortcuts.Name {
    /// Global hotkey for the Spotlight-style search overlay. Default is
    /// ⌥Space (Option + Space) — chosen to rhyme with the system Spotlight
    /// shortcut (⌘Space) while avoiding the conflict. Users can rebind or
    /// clear this in Settings → General → Global Search.
    static let spotlightSearch = Self(
        "spotlightSearch",
        default: .init(.space, modifiers: [.option])
    )
}
```

- [ ] **Step 3: Add the file to the Xcode target**

In Xcode: right-click `hollow` group → Add Files to "hollow"… → select `hollow/Spotlight/KeyboardShortcutsNames.swift` → ensure target membership `hollow` is checked → Add.

- [ ] **Step 4: Build to verify it compiles**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build 2>&1 | tail -10`
Expected: `** BUILD SUCCEEDED **`

- [ ] **Step 5: Commit**

```bash
git add hollow/Spotlight/KeyboardShortcutsNames.swift hollow.xcodeproj/project.pbxproj
git commit -m "feat(spotlight): declare spotlightSearch hotkey with default ⌥Space"
```

---

## Task 3: Coordinator skeleton with failing toggle test

**Files:**
- Create: `hollow/Spotlight/SpotlightCoordinator.swift`
- Create: `hollowTests/SpotlightCoordinatorTests.swift`

- [ ] **Step 1: Write the failing test first**

```swift
// hollowTests/SpotlightCoordinatorTests.swift
import Testing
@testable import hollow

@MainActor
struct SpotlightCoordinatorTests {
    /// Factory: fresh coordinator with a no-op searcher so pure state tests
    /// never accidentally call real hybrid_search.
    private func makeCoordinator() -> SpotlightCoordinator {
        SpotlightCoordinator(searcher: { _, _ in [] })
    }

    @Test
    func toggleFromHiddenShowsPanel() {
        let c = makeCoordinator()
        #expect(c.isVisible == false)
        c.toggle()
        #expect(c.isVisible == true)
    }

    @Test
    func toggleFromShownHidesPanel() {
        let c = makeCoordinator()
        c.toggle()
        #expect(c.isVisible == true)
        c.toggle()
        #expect(c.isVisible == false)
    }

    @Test
    func hideClearsQueryAndResults() {
        let c = makeCoordinator()
        c.toggle()
        c.query = "hello"
        c.results = [.stub(fileName: "a.txt")]
        c.selectedIndex = 0
        c.hide()
        #expect(c.isVisible == false)
        #expect(c.query == "")
        #expect(c.results.isEmpty)
        #expect(c.selectedIndex == 0)
    }
}

extension SearchResult {
    static func stub(fileName: String) -> SearchResult {
        SearchResult(
            fileId: "stub-\(fileName)",
            fileName: fileName,
            currentPath: "/tmp/\(fileName)",
            snippet: "lorem ipsum",
            similarity: -1,
            sources: ["fts"]
        )
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' -only-testing:hollowTests/SpotlightCoordinatorTests test 2>&1 | tail -20`
Expected: FAIL — "Cannot find 'SpotlightCoordinator' in scope".

- [ ] **Step 3: Write the minimal coordinator to make tests compile and pass**

```swift
// hollow/Spotlight/SpotlightCoordinator.swift
import Foundation
import Observation

/// Owns the state of the Spotlight-style global search overlay. Pure state
/// machine — NSPanel lifecycle is wired in a later task. Tests drive this
/// directly with an injected searcher closure so they never hit the real
/// `HollowBridge.shared.hybridSearch`.
@MainActor
@Observable
final class SpotlightCoordinator {
    typealias Searcher = @MainActor (String, UInt32) async -> [SearchResult]

    var isVisible: Bool = false
    var query: String = ""
    var results: [SearchResult] = []
    var selectedIndex: Int = 0

    private let searcher: Searcher
    private var searchTask: Task<Void, Never>?

    init(searcher: @escaping Searcher) {
        self.searcher = searcher
    }

    func toggle() {
        if isVisible {
            hide()
        } else {
            show()
        }
    }

    func show() {
        isVisible = true
    }

    func hide() {
        searchTask?.cancel()
        searchTask = nil
        isVisible = false
        query = ""
        results = []
        selectedIndex = 0
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' -only-testing:hollowTests/SpotlightCoordinatorTests test 2>&1 | tail -20`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add hollow/Spotlight/SpotlightCoordinator.swift hollowTests/SpotlightCoordinatorTests.swift hollow.xcodeproj/project.pbxproj
git commit -m "feat(spotlight): add SpotlightCoordinator state machine skeleton"
```

---

## Task 4: Debounced query → search flow

**Files:**
- Modify: `hollow/Spotlight/SpotlightCoordinator.swift`
- Modify: `hollowTests/SpotlightCoordinatorTests.swift`

- [ ] **Step 1: Add failing tests for query handling**

Append to `SpotlightCoordinatorTests`:

```swift
    @Test
    func emptyQueryProducesEmptyResultsImmediately() async {
        let c = makeCoordinator()
        c.show()
        c.onQueryChange("")
        #expect(c.query == "")
        #expect(c.results.isEmpty)
    }

    @Test
    func nonEmptyQueryCallsSearcherAfterDebounce() async {
        var callLog: [String] = []
        let c = SpotlightCoordinator(searcher: { q, _ in
            callLog.append(q)
            return [.stub(fileName: "\(q).txt")]
        })
        c.show()
        c.onQueryChange("foo")
        // Wait longer than the 250ms debounce
        try? await Task.sleep(for: .milliseconds(400))
        #expect(callLog == ["foo"])
        #expect(c.results.count == 1)
        #expect(c.results[0].fileName == "foo.txt")
    }

    @Test
    func rapidTypingCancelsPreviousSearch() async {
        var callLog: [String] = []
        let c = SpotlightCoordinator(searcher: { q, _ in
            callLog.append(q)
            return [.stub(fileName: "\(q).txt")]
        })
        c.show()
        c.onQueryChange("f")
        try? await Task.sleep(for: .milliseconds(50))
        c.onQueryChange("fo")
        try? await Task.sleep(for: .milliseconds(50))
        c.onQueryChange("foo")
        try? await Task.sleep(for: .milliseconds(400))
        // Only the last query should have reached the searcher — the two
        // earlier tasks were cancelled before their Task.sleep completed.
        #expect(callLog == ["foo"])
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' -only-testing:hollowTests/SpotlightCoordinatorTests test 2>&1 | tail -25`
Expected: FAIL — `onQueryChange` is undefined.

- [ ] **Step 3: Implement `onQueryChange` with debounce**

Add to `SpotlightCoordinator`:

```swift
    /// Debounce window (in milliseconds) between the last keystroke and
    /// actually invoking the searcher. 250ms is the industry-standard
    /// "user stopped typing" threshold and is enough to coalesce rapid
    /// typing in a Spotlight-like panel.
    private static let debounceMs: UInt64 = 250

    /// Call when the TextField's bound `query` changes.
    func onQueryChange(_ newQuery: String) {
        query = newQuery
        searchTask?.cancel()
        searchTask = nil
        selectedIndex = 0

        if newQuery.isEmpty {
            results = []
            return
        }

        searchTask = Task { @MainActor [searcher] in
            try? await Task.sleep(for: .milliseconds(Self.debounceMs))
            if Task.isCancelled { return }
            let hits = await searcher(newQuery, 8)
            if Task.isCancelled { return }
            self.results = hits
            self.selectedIndex = 0
        }
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' -only-testing:hollowTests/SpotlightCoordinatorTests test 2>&1 | tail -20`
Expected: all 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add hollow/Spotlight/SpotlightCoordinator.swift hollowTests/SpotlightCoordinatorTests.swift
git commit -m "feat(spotlight): add 250ms debounced query search with cancellation"
```

---

## Task 5: Keyboard index navigation

**Files:**
- Modify: `hollow/Spotlight/SpotlightCoordinator.swift`
- Modify: `hollowTests/SpotlightCoordinatorTests.swift`

- [ ] **Step 1: Add failing tests for ↑↓ navigation**

Append to `SpotlightCoordinatorTests`:

```swift
    @Test
    func moveSelectionDownAdvances() {
        let c = makeCoordinator()
        c.results = [
            .stub(fileName: "a.txt"),
            .stub(fileName: "b.txt"),
            .stub(fileName: "c.txt"),
        ]
        c.selectedIndex = 0
        c.moveSelectionDown()
        #expect(c.selectedIndex == 1)
        c.moveSelectionDown()
        #expect(c.selectedIndex == 2)
    }

    @Test
    func moveSelectionDownStopsAtLastRow() {
        let c = makeCoordinator()
        c.results = [
            .stub(fileName: "a.txt"),
            .stub(fileName: "b.txt"),
        ]
        c.selectedIndex = 1
        c.moveSelectionDown()
        #expect(c.selectedIndex == 1) // stays pinned, does not wrap
    }

    @Test
    func moveSelectionUpGoesBack() {
        let c = makeCoordinator()
        c.results = [
            .stub(fileName: "a.txt"),
            .stub(fileName: "b.txt"),
            .stub(fileName: "c.txt"),
        ]
        c.selectedIndex = 2
        c.moveSelectionUp()
        #expect(c.selectedIndex == 1)
    }

    @Test
    func moveSelectionUpStopsAtFirstRow() {
        let c = makeCoordinator()
        c.results = [.stub(fileName: "a.txt")]
        c.selectedIndex = 0
        c.moveSelectionUp()
        #expect(c.selectedIndex == 0) // stays pinned
    }

    @Test
    func selectionNoopWhenResultsEmpty() {
        let c = makeCoordinator()
        c.results = []
        c.selectedIndex = 0
        c.moveSelectionDown()
        c.moveSelectionUp()
        #expect(c.selectedIndex == 0)
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' -only-testing:hollowTests/SpotlightCoordinatorTests test 2>&1 | tail -20`
Expected: FAIL — `moveSelectionDown` / `moveSelectionUp` undefined.

- [ ] **Step 3: Implement navigation methods**

Add to `SpotlightCoordinator`:

```swift
    func moveSelectionDown() {
        guard !results.isEmpty else { return }
        selectedIndex = min(selectedIndex + 1, results.count - 1)
    }

    func moveSelectionUp() {
        guard !results.isEmpty else { return }
        selectedIndex = max(selectedIndex - 1, 0)
    }

    /// Currently selected result, if any.
    var selectedResult: SearchResult? {
        guard results.indices.contains(selectedIndex) else { return nil }
        return results[selectedIndex]
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' -only-testing:hollowTests/SpotlightCoordinatorTests test 2>&1 | tail -20`
Expected: all 11 tests pass.

- [ ] **Step 5: Commit**

```bash
git add hollow/Spotlight/SpotlightCoordinator.swift hollowTests/SpotlightCoordinatorTests.swift
git commit -m "feat(spotlight): add clamped ↑↓ selection navigation"
```

---

## Task 6: Action methods (open file, reveal in Finder)

**Files:**
- Modify: `hollow/Spotlight/SpotlightCoordinator.swift`

These are thin AppKit wrappers, not worth unit-testing — `NSWorkspace.open` is what we want to verify is being invoked but `NSWorkspace` is a hard-to-mock global. Instead we rely on manual testing later.

- [ ] **Step 1: Add action methods**

Add to `SpotlightCoordinator`:

```swift
    /// Execute the "primary" action (↵) on the currently-selected result:
    /// open the file with the system's default app, then hide the panel.
    func openSelected() {
        guard let result = selectedResult else { return }
        let url = URL(fileURLWithPath: result.currentPath)
        NSWorkspace.shared.open(url)
        hide()
    }

    /// Execute the "secondary" action (⌘↵) on the currently-selected result:
    /// reveal the file in a new Finder window, then hide the panel.
    func revealSelected() {
        guard let result = selectedResult else { return }
        let url = URL(fileURLWithPath: result.currentPath)
        NSWorkspace.shared.activateFileViewerSelecting([url])
        hide()
    }
```

Also add the missing `import AppKit` at the top of the file if not already there:

```swift
import AppKit
import Foundation
import Observation
```

- [ ] **Step 2: Build to verify compilation**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build 2>&1 | tail -15`
Expected: `** BUILD SUCCEEDED **`

- [ ] **Step 3: Commit**

```bash
git add hollow/Spotlight/SpotlightCoordinator.swift
git commit -m "feat(spotlight): add openSelected / revealSelected action methods"
```

---

## Task 7: `SpotlightResultRow` view

**Files:**
- Create: `hollow/Spotlight/SpotlightResultRow.swift`

- [ ] **Step 1: Write the row view**

```swift
// hollow/Spotlight/SpotlightResultRow.swift
import AppKit
import SwiftUI

/// One row in the Spotlight search results list. 52pt high, icon + filename
/// + snippet. Selection is driven by the parent view (no hover state of its
/// own — selection and hover are unified in the coordinator).
struct SpotlightResultRow: View {
    let result: SearchResult
    let isSelected: Bool

    var body: some View {
        HStack(spacing: 12) {
            fileIcon
                .frame(width: 36, height: 36)

            VStack(alignment: .leading, spacing: 2) {
                Text(result.fileName)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(isSelected ? Color.white : .primary)
                    .lineLimit(1)
                    .truncationMode(.middle)

                if !result.snippet.isEmpty {
                    Text(snippetPlain(result.snippet))
                        .font(.system(size: 12))
                        .foregroundStyle(isSelected ? Color.white.opacity(0.85) : .secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }

            Spacer(minLength: 0)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 8)
        .frame(height: 52)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 6)
                .fill(isSelected ? Color.accentColor : .clear)
                .padding(.horizontal, 8)
        )
        .contentShape(Rectangle())
    }

    /// Native file icon for the result's on-disk path. Falls back to a generic
    /// doc icon if the file no longer exists (e.g. moved after indexing).
    private var fileIcon: some View {
        let ws = NSWorkspace.shared
        let nsImage: NSImage
        if FileManager.default.fileExists(atPath: result.currentPath) {
            nsImage = ws.icon(forFile: result.currentPath)
        } else {
            nsImage = ws.icon(for: .data)
        }
        return Image(nsImage: nsImage)
            .resizable()
            .aspectRatio(contentMode: .fit)
    }

    /// FTS5 snippets are returned with `<b>...</b>` around hit terms. For v1
    /// we strip the tags and show plain text; rich highlighting can be added
    /// as an enhancement once the base layout is proven.
    private func snippetPlain(_ s: String) -> String {
        s.replacingOccurrences(of: "<b>", with: "")
         .replacingOccurrences(of: "</b>", with: "")
    }
}
```

- [ ] **Step 2: Add to Xcode target**

Xcode: right-click `Spotlight` group → Add Files → `hollow/Spotlight/SpotlightResultRow.swift` → target `hollow` checked → Add.

- [ ] **Step 3: Build to verify compilation**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build 2>&1 | tail -10`
Expected: `** BUILD SUCCEEDED **`

- [ ] **Step 4: Commit**

```bash
git add hollow/Spotlight/SpotlightResultRow.swift hollow.xcodeproj/project.pbxproj
git commit -m "feat(spotlight): add SpotlightResultRow 52pt-high row component"
```

---

## Task 8: `SpotlightView` — input + results + keyboard bindings

**Files:**
- Create: `hollow/Spotlight/SpotlightView.swift`

- [ ] **Step 1: Write the main view**

```swift
// hollow/Spotlight/SpotlightView.swift
import SwiftUI

/// SwiftUI content of the Spotlight panel. Hosted by `NSHostingView` inside
/// `SpotlightPanel`. Reads state directly from an observed coordinator and
/// drives all actions through it.
struct SpotlightView: View {
    @Bindable var coordinator: SpotlightCoordinator
    @FocusState private var fieldFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            searchField
            if !coordinator.query.isEmpty || !coordinator.results.isEmpty {
                Divider()
                resultsSection
            } else {
                Divider()
                emptyState
            }
        }
        .frame(width: 680)
        .background(.ultraThinMaterial)
        .clipShape(RoundedRectangle(cornerRadius: 10))
        .onAppear { fieldFocused = true }
        .onChange(of: coordinator.isVisible) { _, visible in
            if visible { fieldFocused = true }
        }
        // ↵ — open the selected file
        .onKeyPress(.return) {
            if coordinator.selectedResult != nil {
                coordinator.openSelected()
                return .handled
            }
            return .ignored
        }
        // ↓ — move selection down
        .onKeyPress(.downArrow) {
            coordinator.moveSelectionDown()
            return .handled
        }
        // ↑ — move selection up
        .onKeyPress(.upArrow) {
            coordinator.moveSelectionUp()
            return .handled
        }
        // ESC — hide
        .onExitCommand {
            coordinator.hide()
        }
    }

    private var searchField: some View {
        HStack(spacing: 12) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 18, weight: .regular))
                .foregroundStyle(.secondary)

            TextField("Search hollow...", text: Binding(
                get: { coordinator.query },
                set: { coordinator.onQueryChange($0) }
            ))
            .textFieldStyle(.plain)
            .font(.system(size: 22, weight: .regular))
            .focused($fieldFocused)
        }
        .padding(.horizontal, 24)
        .frame(height: 60)
    }

    private var resultsSection: some View {
        VStack(spacing: 0) {
            if coordinator.results.isEmpty {
                Text("No matches")
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
                    .frame(height: 52)
            } else {
                ForEach(Array(coordinator.results.enumerated()), id: \.offset) { index, result in
                    SpotlightResultRow(
                        result: result,
                        isSelected: index == coordinator.selectedIndex
                    )
                    .onTapGesture {
                        coordinator.selectedIndex = index
                        coordinator.openSelected()
                    }
                    .onHover { hovering in
                        if hovering { coordinator.selectedIndex = index }
                    }
                    if index < coordinator.results.count - 1 {
                        Divider().padding(.horizontal, 20)
                    }
                }
            }
        }
    }

    private var emptyState: some View {
        Text("Start typing to search...")
            .font(.system(size: 13))
            .foregroundStyle(.tertiary)
            .frame(maxWidth: .infinity)
            .frame(height: 52)
    }
}
```

Note on ⌘↵: SwiftUI's `.onKeyPress(.return)` does not natively report modifiers on macOS in the way we'd like, and wrapping the whole view in a `.keyboardShortcut(.return, modifiers: .command)` button is the cleanest path. We add a hidden button for ⌘↵:

- [ ] **Step 2: Add hidden `⌘↵` reveal button**

Inside the outer `VStack(spacing: 0) { ... }` in `body`, add at the top (or anywhere that's part of the view hierarchy):

```swift
        // Hidden button so `.keyboardShortcut(.return, modifiers: .command)`
        // gets picked up by the focus system. `.onKeyPress` doesn't expose
        // modifier chords reliably on macOS, and a hidden button is the
        // stable pattern used elsewhere in SwiftUI for cmd-return style.
        Button("Reveal") { coordinator.revealSelected() }
            .keyboardShortcut(.return, modifiers: .command)
            .hidden()
            .frame(width: 0, height: 0)
```

- [ ] **Step 3: Add to Xcode target and build**

Xcode: add `hollow/Spotlight/SpotlightView.swift` to target `hollow`.
Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build 2>&1 | tail -15`
Expected: `** BUILD SUCCEEDED **`.

- [ ] **Step 4: Commit**

```bash
git add hollow/Spotlight/SpotlightView.swift hollow.xcodeproj/project.pbxproj
git commit -m "feat(spotlight): add SpotlightView with input, results, keyboard bindings"
```

---

## Task 9: `SpotlightPanel` — the NSPanel subclass

**Files:**
- Create: `hollow/Spotlight/SpotlightPanel.swift`

- [ ] **Step 1: Write the NSPanel subclass**

```swift
// hollow/Spotlight/SpotlightPanel.swift
import AppKit
import SwiftUI

/// Borderless, non-activating HUD-style panel that hosts the Spotlight
/// search UI. Made key so the embedded `TextField` can receive focus, but
/// does NOT activate the app — clicking into the panel from another app
/// leaves that app's windows as they were.
final class SpotlightPanel: NSPanel {
    init(rootView: SpotlightView) {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 680, height: 60),
            styleMask: [.borderless, .nonactivatingPanel, .hudWindow],
            backing: .buffered,
            defer: false
        )
        self.isOpaque = false
        self.backgroundColor = .clear
        self.hasShadow = true
        self.level = .floating
        self.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        self.isMovable = false
        self.hidesOnDeactivate = false
        self.animationBehavior = .utilityWindow

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        let container = NSView()
        container.addSubview(host)
        NSLayoutConstraint.activate([
            host.topAnchor.constraint(equalTo: container.topAnchor),
            host.bottomAnchor.constraint(equalTo: container.bottomAnchor),
            host.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: container.trailingAnchor),
        ])
        self.contentView = container
    }

    /// Required so the embedded SwiftUI `TextField` can become first responder.
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }

    /// Position the panel centered horizontally on the current main screen,
    /// with its top 35% down from the top of the screen's visible frame.
    func positionCentered() {
        guard let screen = NSScreen.main ?? NSScreen.screens.first else { return }
        let visible = screen.visibleFrame
        let size = self.frame.size
        let x = visible.midX - size.width / 2
        let y = visible.maxY - size.height - visible.height * 0.35
        self.setFrameOrigin(NSPoint(x: x, y: y))
    }
}
```

- [ ] **Step 2: Add to target and build**

Xcode: add `SpotlightPanel.swift` to target `hollow`.
Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build 2>&1 | tail -10`
Expected: `** BUILD SUCCEEDED **`.

- [ ] **Step 3: Commit**

```bash
git add hollow/Spotlight/SpotlightPanel.swift hollow.xcodeproj/project.pbxproj
git commit -m "feat(spotlight): add SpotlightPanel NSPanel subclass (HUD, non-activating)"
```

---

## Task 10: Wire coordinator ↔ panel lifecycle via injected presenters

**Files:**
- Modify: `hollow/Spotlight/SpotlightCoordinator.swift`

We inject panel-presentation as two closures (`presenter` and `dismisser`) so that:
- Production code (via a convenience initializer) passes in real `SpotlightPanel.makeKeyAndOrderFront` / `orderOut`
- Tests keep using `init(searcher:)` which defaults the closures to no-ops — `NSPanel` is never constructed in the test host, eliminating any run-loop / AppKit-lifecycle risk

- [ ] **Step 1: Update `SpotlightCoordinator` with presenter closures**

Replace the existing `init(searcher:)` and `show()` / `hide()` block with:

```swift
    typealias PanelAction = @MainActor () -> Void

    private let presenter: PanelAction
    private let dismisser: PanelAction

    init(
        searcher: @escaping Searcher,
        presenter: @escaping PanelAction = {},
        dismisser: @escaping PanelAction = {}
    ) {
        self.searcher = searcher
        self.presenter = presenter
        self.dismisser = dismisser
    }

    func show() {
        isVisible = true
        presenter()
    }

    func hide() {
        searchTask?.cancel()
        searchTask = nil
        isVisible = false
        query = ""
        results = []
        selectedIndex = 0
        dismisser()
    }
```

The tests continue to call `SpotlightCoordinator(searcher: { _, _ in [] })` unchanged — default no-op closures mean `show()` / `hide()` don't touch AppKit.

- [ ] **Step 2: Add production convenience init**

Append to `SpotlightCoordinator`:

```swift
    /// Production factory: wires the real hybrid search + a real `SpotlightPanel`.
    /// The panel and its key-resigned observer are owned by the factory-level
    /// holder pair below; the coordinator only knows how to ask them to
    /// appear / disappear.
    static func makeProduction() -> SpotlightCoordinator {
        // Lazily constructed on first `show()`; held for the lifetime of the
        // coordinator so we don't rebuild it per toggle.
        final class PanelHolder {
            var panel: SpotlightPanel?
            var resignObserver: NSObjectProtocol?
        }
        let holder = PanelHolder()

        // Declared as `var` first so we can reference `coordinator` inside
        // the presenter closure (circular dep: closure needs coordinator,
        // coordinator needs closure).
        var coordinator: SpotlightCoordinator!

        let searcher: Searcher = { query, limit in
            await withCheckedContinuation { cont in
                DispatchQueue.global(qos: .userInitiated).async {
                    let hits = HollowBridge.shared.hybridSearch(query: query, limit: limit)
                    cont.resume(returning: hits)
                }
            }
        }

        let presenter: PanelAction = { [holder] in
            if holder.panel == nil {
                let view = SpotlightView(coordinator: coordinator)
                let p = SpotlightPanel(rootView: view)
                holder.panel = p
                holder.resignObserver = NotificationCenter.default.addObserver(
                    forName: NSWindow.didResignKeyNotification,
                    object: p,
                    queue: .main
                ) { _ in
                    Task { @MainActor in coordinator.hide() }
                }
            }
            guard let panel = holder.panel else { return }
            panel.positionCentered()
            panel.setContentSize(NSSize(width: 680, height: 60))
            panel.makeKeyAndOrderFront(nil)
        }

        let dismisser: PanelAction = { [holder] in
            holder.panel?.orderOut(nil)
        }

        coordinator = SpotlightCoordinator(
            searcher: searcher,
            presenter: presenter,
            dismisser: dismisser
        )
        return coordinator
    }
```

- [ ] **Step 3: Build to verify compilation**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build 2>&1 | tail -15`
Expected: `** BUILD SUCCEEDED **`.

- [ ] **Step 4: Verify tests still pass**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' -only-testing:hollowTests/SpotlightCoordinatorTests test 2>&1 | tail -20`
Expected: all 11 tests still pass. The tests use `init(searcher:)` which defaults `presenter` / `dismisser` to no-ops — no AppKit touched.

- [ ] **Step 5: Commit**

```bash
git add hollow/Spotlight/SpotlightCoordinator.swift
git commit -m "feat(spotlight): wire coordinator ↔ SpotlightPanel via injected presenters"
```

---

## Task 11: Mount in `hollowApp` + register hotkey handler

**Files:**
- Modify: [hollow/hollowApp.swift](../../../hollow/hollowApp.swift)

We use a top-level (module-scope) holder rather than a SwiftUI `@State` property because the `KeyboardShortcuts.onKeyDown` callback is registered in `init()` and must reference a stable Swift object — `@State` property wrappers cannot be captured from `init()` context, and `@State` wrapper values are not stable across view re-creations.

- [ ] **Step 1: Add the import and module-level coordinator holder**

At the top of `hollowApp.swift`, add:

```swift
import KeyboardShortcuts
```

At the bottom of `hollowApp.swift` (outside the `hollowApp` struct, at module scope), add:

```swift
/// Process-wide singleton for the Spotlight search coordinator. Declared at
/// module scope because the `KeyboardShortcuts.onKeyDown` callback is
/// registered in `hollowApp.init()` and needs a stable object reference that
/// a SwiftUI `@State` property cannot provide. There is only ever one global
/// search overlay for the whole app, so a shared instance is the right fit.
@MainActor
let spotlightCoordinator: SpotlightCoordinator = SpotlightCoordinator.makeProduction()
```

- [ ] **Step 2: Register the hotkey and app-deactivate observer in `init()`**

Replace the existing `hollowApp.init()` body with:

```swift
    init() {
        // Apply language override before any UI renders
        let lang = UserDefaults.standard.string(forKey: "appLanguage") ?? ""
        if !lang.isEmpty {
            UserDefaults.standard.set([lang], forKey: "AppleLanguages")
        } else {
            UserDefaults.standard.removeObject(forKey: "AppleLanguages")
        }

        // Register the global Spotlight search hotkey. Uses the shared
        // top-level coordinator so the closure doesn't capture a SwiftUI
        // state wrapper.
        KeyboardShortcuts.onKeyDown(for: .spotlightSearch) {
            Task { @MainActor in
                spotlightCoordinator.toggle()
            }
        }

        // Hide the overlay when the app is deactivated (user clicks into
        // another app). This is a belt-and-braces in addition to the
        // NSPanel didResignKey observer, since non-activating panels can
        // briefly keep key status during cross-app switches.
        NotificationCenter.default.addObserver(
            forName: NSApplication.didResignActiveNotification,
            object: nil,
            queue: .main
        ) { _ in
            Task { @MainActor in
                spotlightCoordinator.hide()
            }
        }
    }
```

- [ ] **Step 3: Build and run manually**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build 2>&1 | tail -15`
Expected: `** BUILD SUCCEEDED **`.

Then launch the app from Xcode (⌘R) and verify:
- Press ⌥Space → panel appears in the top-center area of the screen
- Type "test" (or any 3+ char string you have indexed) → results appear after ~250ms
- Press ↓ then ↓ → second result highlights
- Press ↵ → file opens + panel closes
- Press ⌥Space again → panel appears empty
- Press ESC → panel closes
- Press ⌥Space, click outside panel → panel closes

- [ ] **Step 4: Commit**

```bash
git add hollow/hollowApp.swift
git commit -m "feat(spotlight): mount SpotlightCoordinator and register ⌥Space hotkey"
```

---

## Task 12: Settings → General → Global Search

**Files:**
- Modify: [hollow/SettingsView.swift](../../../hollow/SettingsView.swift)

- [ ] **Step 1: Add KeyboardShortcuts import**

At the top of `SettingsView.swift`:

```swift
import KeyboardShortcuts
```

- [ ] **Step 2: Add a new Section in `GeneralSettingsView.generalForm`**

Insert this `Section` between the existing `"Menu Bar"` section and the `"Language"` section (around line 105 in the current file):

```swift
            Section("Global Search") {
                KeyboardShortcuts.Recorder(
                    String(localized: "Search hotkey:"),
                    name: .spotlightSearch
                )
                Text("Press this shortcut from anywhere to open the Hollow search overlay. Click the ⓧ in the recorder to disable it entirely.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
```

- [ ] **Step 3: Build and verify in Settings**

Run: `xcodebuild -project hollow.xcodeproj -scheme hollow -configuration Debug build 2>&1 | tail -10`
Expected: `** BUILD SUCCEEDED **`.

Launch the app, open Settings → General. Verify:
- New "Global Search" section visible with a recorder showing "⌥Space"
- Click the recorder and record a new shortcut (e.g. ⌃⌥S) — it saves
- New shortcut works immediately (no app restart needed)
- Click ⓧ in the recorder → shortcut cleared, ⌥Space no longer triggers
- Re-record ⌥Space to restore the default

- [ ] **Step 4: Commit**

```bash
git add hollow/SettingsView.swift
git commit -m "feat(spotlight): add Global Search hotkey recorder in Settings"
```

---

## Task 13: Manual verification pass

**Files:** none — this is a human checklist.

Run through the full spec's manual test checklist. For each, mark ✅ or ❌ in a scratch note.

- [ ] **Step 1: Run the full build + test suite**

```bash
xcodebuild -project hollow.xcodeproj -scheme hollow -destination 'platform=macOS' test 2>&1 | tail -30
```
Expected: all tests pass, including the 11 new `SpotlightCoordinatorTests`.

- [ ] **Step 2: Manual test checklist**

Launch the app. Verify each item (from the spec's 测试策略 → 手动验证清单 section):

- [ ] ⌥Space in Safari → panel appears
- [ ] ⌥Space in Xcode → panel appears
- [ ] ⌥Space in Finder → panel appears
- [ ] ESC closes panel
- [ ] Click outside panel closes it
- [ ] ⌥Space again while panel open → closes (toggle)
- [ ] Switching to another app (⌘Tab) closes panel
- [ ] ↑↓ keyboard navigation, cannot go past first/last row
- [ ] ↵ opens PDF in default app, panel closes
- [ ] ⌘↵ reveals file in Finder, panel closes
- [ ] Input method switch (ABC ↔ Pinyin) doesn't lose characters
- [ ] If Alfred/Raycast also running on ⌥Space: either conflict warning in Settings recorder, or one of them gets the event — document which
- [ ] External display as primary: panel appears on the display with `NSScreen.main`
- [ ] Stage Manager: panel floats over the active stage
- [ ] Full-screen app (e.g. full-screen Safari): panel appears on top
- [ ] Rapid typing triggers only one search (check Xcode console for `hybrid_search` call count — should be one per typing burst, not per keystroke)
- [ ] Empty query shows "Start typing to search..." placeholder
- [ ] `query.count < 3` (ASCII): results empty because FTS5 trigram needs 3+ chars. This is expected — optionally add a "Type at least 3 characters" hint in a future iteration.
- [ ] Query with no matches shows "No matches" row
- [ ] After executing an action, next ⌥Space opens with empty query
- [ ] Settings recorder: new shortcut takes effect without restart
- [ ] Settings recorder cleared: ⌥Space no longer triggers
- [ ] Settings recorder re-record ⌥Space: restored

- [ ] **Step 3: Update [docs/engineering-status.md](../../../docs/engineering-status.md)**

Add a new section under "已完成的里程碑" after the current `Batch 3` section:

```markdown
### Spotlight 风格全局搜索浮层（2026-04-13）

- [x] `sindresorhus/KeyboardShortcuts` SPM 依赖
- [x] `SpotlightCoordinator` 状态机（@Observable 单例，250ms debounce，↑↓ 导航）
- [x] `SpotlightPanel` NSPanel 子类（.hudWindow + .nonactivatingPanel + .floating）
- [x] `SpotlightView` SwiftUI 视图（⌥Space toggle，↵ Open，⌘↵ Reveal，ESC/点击外部/切 App 关闭）
- [x] `SpotlightResultRow` 紧凑两行（48pt 图标 + 文件名 + snippet）
- [x] Settings → General → Global Search 可录制/禁用快捷键
- [x] 11 个 Swift Testing 单测覆盖状态机 + debounce + 选中导航

**v1 简化**：
- 结果行右侧未显示相对时间（SearchResult FFI 无 timestamp 字段）
- 空查询状态显示 placeholder 文字，未列出最近文件（IngestionService.recentFiles 是纯文件名数组）

**相关文档**：
- Spec: `docs/superpowers/specs/2026-04-13-spotlight-search-overlay-design.md`
- Plan: `docs/superpowers/plans/2026-04-13-spotlight-search-overlay.md`
```

- [ ] **Step 4: Commit**

```bash
git add docs/engineering-status.md
git commit -m "docs: record Spotlight search overlay milestone in engineering status"
```

---

## Summary

13 tasks total:
1. Add KeyboardShortcuts SPM dep
2. Declare `.spotlightSearch` hotkey name
3. Coordinator skeleton + toggle tests (TDD)
4. Debounced search flow + tests (TDD)
5. ↑↓ navigation + tests (TDD)
6. Open / Reveal action methods
7. `SpotlightResultRow` view
8. `SpotlightView` with keyboard bindings
9. `SpotlightPanel` NSPanel subclass
10. Wire coordinator ↔ panel lifecycle
11. Mount in `hollowApp` + hotkey handler
12. Settings recorder in General tab
13. Manual verification + engineering-status update

Tasks 3–5 are strict TDD (red → green → refactor → commit). Tasks 6–12 are straightforward scaffolding where unit tests would not add value (AppKit panel lifecycle, SwiftUI views, app wiring). Task 13 is a human verification pass.

Zero Rust changes. All code lives under `hollow/Spotlight/` except a handful of line-level insertions in `hollowApp.swift` and `SettingsView.swift`.
