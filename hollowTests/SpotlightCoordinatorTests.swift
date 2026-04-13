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
