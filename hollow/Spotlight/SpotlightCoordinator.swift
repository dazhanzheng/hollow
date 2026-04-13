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
}
