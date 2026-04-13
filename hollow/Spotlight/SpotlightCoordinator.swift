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
