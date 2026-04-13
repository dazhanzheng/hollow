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
