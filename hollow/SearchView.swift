import SwiftUI

struct SearchView: View {
    @State private var query = ""
    @State private var results: [SearchResult] = []
    @State private var isSearching = false

    var body: some View {
        VStack(spacing: 0) {
            // Search bar
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("Search files…", text: $query)
                    .textFieldStyle(.plain)
                    .onSubmit { performSearch() }
                if !query.isEmpty {
                    Button {
                        query = ""
                        results = []
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

            // Results
            if results.isEmpty && !query.isEmpty && !isSearching {
                ContentUnavailableView.search(text: query)
            } else {
                List(results, id: \.fileId) { result in
                    SearchResultRow(result: result)
                }
                .listStyle(.plain)
            }
        }
        .frame(minWidth: 500, minHeight: 400)
        .onChange(of: query) {
            if query.count >= 3 {
                performSearch()
            } else if query.isEmpty {
                results = []
            }
        }
    }

    private func performSearch() {
        isSearching = true
        let currentQuery = query
        DispatchQueue.global(qos: .userInitiated).async {
            let searchResults = HollowBridge.shared.search(
                query: currentQuery,
                limit: 50
            )
            DispatchQueue.main.async {
                if query == currentQuery {
                    results = searchResults
                    isSearching = false
                }
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
            }
            Text(result.currentPath)
                .font(.caption)
                .foregroundStyle(.tertiary)
                .lineLimit(1)
                .truncationMode(.middle)
            Text(snippetPlainText(result.snippet))
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(3)
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
