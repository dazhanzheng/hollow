import SwiftUI

struct DatabaseBrowserView: View {
    @State private var files: [FileRecord] = []
    @State private var selectedFileId: String?
    @State private var searchText = ""

    private var selectedFile: FileRecord? {
        guard let id = selectedFileId else { return nil }
        return files.first { $0.id == id }
    }

    var body: some View {
        NavigationSplitView {
            List(filteredFiles, id: \.id, selection: $selectedFileId) { file in
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        statusDot(file.status)
                        Text(file.fileName)
                            .fontWeight(.medium)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    HStack(spacing: 8) {
                        Label(formatBytes(file.sizeBytes), systemImage: "doc")
                        if let mime = file.mimeType {
                            Text(mime)
                        }
                        Text(file.status)
                            .foregroundStyle(statusColor(file.status))
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .labelStyle(.titleOnly)
                }
                .padding(.vertical, 2)
            }
            .searchable(text: $searchText, prompt: "Filter by name...")
            .navigationSplitViewColumnWidth(min: 260, ideal: 320)
        } detail: {
            if let file = selectedFile {
                ScrollView {
                    DetailPanel(file: file)
                }
            } else {
                ContentUnavailableView(
                    "No Selection",
                    systemImage: "doc.text.magnifyingglass",
                    description: Text("Select a file to view details")
                )
            }
        }
        .frame(minWidth: 720, minHeight: 480)
        .onAppear { reload() }
        .toolbar {
            ToolbarItem {
                Button(action: reload) {
                    Image(systemName: "arrow.clockwise")
                }
                .help("Refresh")
            }
            ToolbarItem {
                Text("\(files.count) records")
                    .foregroundStyle(.secondary)
                    .font(.caption)
                    .monospacedDigit()
            }
        }
    }

    private var filteredFiles: [FileRecord] {
        if searchText.isEmpty { return files }
        let query = searchText.lowercased()
        return files.filter {
            $0.fileName.lowercased().contains(query) ||
            $0.currentPath.lowercased().contains(query) ||
            $0.status.lowercased().contains(query)
        }
    }

    private func reload() {
        files = HollowBridge.shared.listFiles(limit: UInt32.max, offset: 0)
    }

    private func statusDot(_ status: String) -> some View {
        Circle()
            .fill(statusColor(status))
            .frame(width: 7, height: 7)
    }

    private func statusColor(_ status: String) -> Color {
        switch status {
        case "indexed": .green
        case "pending": .orange
        case "missing": .red
        default: .gray
        }
    }

    private func formatBytes(_ bytes: Int64) -> String {
        let formatter = ByteCountFormatter()
        formatter.countStyle = .file
        return formatter.string(fromByteCount: bytes)
    }
}

private struct DetailPanel: View {
    let file: FileRecord

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            section("Identity") {
                row("ID", file.id)
                row("Status", file.status)
                row("Inode", file.inode.map { String($0) } ?? "—")
            }

            section("File") {
                row("Name", file.fileName)
                row("Extension", file.extension ?? "—")
                row("MIME", file.mimeType ?? "—")
                row("Size", ByteCountFormatter.string(fromByteCount: file.sizeBytes, countStyle: .file))
            }

            section("Paths") {
                row("Current", file.currentPath)
                row("Original", file.originalPath)
            }

            section("Timestamps") {
                row("Created", file.createdAt)
                row("Modified", file.modifiedAt)
                row("Ingested", file.ingestedAt)
            }

            section("Hashes") {
                row("Quick Hash", file.quickHash.isEmpty ? "—" : file.quickHash)
                row("Full Hash", file.hash.isEmpty ? "—" : file.hash)
            }
        }
        .padding(20)
        .textSelection(.enabled)
    }

    private func section(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.headline)
            content()
            Divider()
        }
    }

    private func row(_ label: String, _ value: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Text(label)
                .foregroundStyle(.secondary)
                .frame(width: 90, alignment: .trailing)
            Text(value)
                .font(.system(.body, design: .monospaced))
                .lineLimit(nil)
        }
        .font(.callout)
    }
}
