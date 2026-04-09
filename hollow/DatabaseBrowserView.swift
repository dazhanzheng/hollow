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
        HSplitView {
            // Left: file list
            VStack(spacing: 0) {
                // Search bar
                TextField("Filter by name...", text: $searchText)
                    .textFieldStyle(.roundedBorder)
                    .padding(8)

                // Table
                List(filteredFiles, id: \.id, selection: $selectedFileId) { file in
                    VStack(alignment: .leading, spacing: 2) {
                        HStack {
                            statusDot(file.status)
                            Text(file.fileName)
                                .fontWeight(.medium)
                                .lineLimit(1)
                        }
                        HStack(spacing: 8) {
                            Text(formatBytes(file.sizeBytes))
                            Text(file.mimeType ?? "unknown")
                            Text(file.status)
                                .foregroundStyle(statusColor(file.status))
                        }
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 2)
                }
            }
            .frame(minWidth: 300)

            // Right: detail panel
            if let file = selectedFile {
                ScrollView {
                    DetailPanel(file: file)
                }
                .frame(minWidth: 350)
            } else {
                VStack {
                    Text("Select a file to view details")
                        .foregroundStyle(.secondary)
                }
                .frame(minWidth: 350)
            }
        }
        .frame(minWidth: 700, minHeight: 450)
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
            .frame(width: 6, height: 6)
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
        VStack(alignment: .leading, spacing: 12) {
            Group {
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
        }
        .padding()
        .textSelection(.enabled)
    }

    private func section(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.headline)
                .padding(.top, 4)
            content()
            Divider()
        }
    }

    private func row(_ label: String, _ value: String) -> some View {
        HStack(alignment: .top) {
            Text(label)
                .foregroundStyle(.secondary)
                .frame(width: 90, alignment: .trailing)
            Text(value)
                .font(.system(.body, design: .monospaced))
                .lineLimit(nil)
        }
        .font(.caption)
    }
}
