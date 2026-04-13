// hollow/Spotlight/SpotlightCoordinator.swift
import AppKit
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

    typealias PanelAction = @MainActor () -> Void

    private let searcher: Searcher
    private let presenter: PanelAction
    private let dismisser: PanelAction
    private var searchTask: Task<Void, Never>?

    init(
        searcher: @escaping Searcher,
        presenter: @escaping PanelAction = {},
        dismisser: @escaping PanelAction = {}
    ) {
        self.searcher = searcher
        self.presenter = presenter
        self.dismisser = dismisser
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
}
