import SwiftUI

struct SearchView: View {
    @State private var query = ""
    @State private var results: [SearchResult] = []
    @State private var isSearching = false
    @State private var hasSearched = false

    var body: some View {
        VStack(spacing: 0) {
            // Search bar — always pinned at top
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("Search files… (press Return)", text: $query)
                    .textFieldStyle(.plain)
                    .onSubmit { performSearch() }
                    .onChange(of: query) {
                        // Reset searched state when user edits query
                        hasSearched = false
                    }
                if !query.isEmpty {
                    Button {
                        query = ""
                        results = []
                        hasSearched = false
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(12)
            .background(.bar)

            Divider()

            // Results area — always fills remaining space
            if !hasSearched {
                // Initial state: empty prompt
                VStack {
                    Spacer()
                    Text("Type a query and press Return to search")
                        .font(.subheadline)
                        .foregroundStyle(.tertiary)
                    Spacer()
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if results.isEmpty {
                // Searched but no results
                VStack {
                    Spacer()
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 32))
                        .foregroundStyle(.tertiary)
                    Text("No results for \"\(query)\"")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(results, id: \.fileId) { result in
                    SearchResultRow(result: result)
                }
                .listStyle(.plain)
            }
        }
        .frame(minWidth: 500, minHeight: 400)
    }

    private func performSearch() {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        guard trimmed.count >= 2 else { return }
        isSearching = true
        hasSearched = true
        DispatchQueue.global(qos: .userInitiated).async {
            let searchResults = HollowBridge.shared.hybridSearch(
                query: trimmed,
                limit: 50
            )
            DispatchQueue.main.async {
                results = searchResults
                isSearching = false
            }
        }
    }
}

private struct SearchResultRow: View {
    let result: SearchResult

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Image(systemName: "doc.text")
                    .foregroundStyle(.tint)
                Text(result.fileName)
                    .font(.body.weight(.medium))
                Spacer()
                // Source tags
                ForEach(result.sources, id: \.self) { source in
                    SourceTag(source: source)
                }
                // Cosine similarity (only if embedding matched)
                if result.similarity >= 0 {
                    SimilarityBadge(value: result.similarity)
                }
            }
            Text(result.currentPath)
                .font(.caption)
                .foregroundStyle(.tertiary)
                .lineLimit(1)
                .truncationMode(.middle)
            if !result.snippet.isEmpty {
                Text(snippetPlainText(result.snippet))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
            }
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
        .onTapGesture {
            NSWorkspace.shared.activateFileViewerSelecting(
                [URL(fileURLWithPath: result.currentPath)]
            )
        }
    }

    private func snippetPlainText(_ snippet: String) -> String {
        snippet.replacingOccurrences(of: "<b>", with: "")
               .replacingOccurrences(of: "</b>", with: "")
    }
}

private struct SourceTag: View {
    let source: String

    var body: some View {
        let isKeyword = source == "fts"
        Text(isKeyword ? "keyword" : "semantic")
            .font(.caption2.weight(.medium))
            .padding(.horizontal, 5)
            .padding(.vertical, 1)
            .background(isKeyword ? Color.blue.opacity(0.15) : Color.purple.opacity(0.15),
                        in: Capsule())
            .foregroundStyle(isKeyword ? Color.blue : Color.purple)
    }
}

private struct SimilarityBadge: View {
    let value: Double

    var body: some View {
        let color: Color = value > 0.8 ? .green : value > 0.6 ? .orange : .gray
        Text("\(Int(value * 100))%")
            .font(.caption2.monospacedDigit())
            .foregroundStyle(color)
    }
}
