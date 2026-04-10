import SwiftUI

struct DatabaseBrowserView: View {
    @Environment(IngestionService.self) private var ingestion
    @State private var files: [FileRecord] = []
    @State private var selectedFileId: String?
    @State private var searchText = ""

    private var selectedFile: FileRecord? {
        guard let id = selectedFileId else { return nil }
        return files.first { $0.id == id }
    }

    /// Counts of each status across the current snapshot, for the summary bar.
    private var statusCounts: [(String, Int)] {
        var counts: [String: Int] = [:]
        for f in files { counts[f.status, default: 0] += 1 }
        // Canonical display order; unknown statuses appended at the end.
        let order = ["indexed", "pending", "extracting", "unsupported", "extract_failed", "missing"]
        var result: [(String, Int)] = []
        for status in order {
            if let n = counts[status], n > 0 { result.append((status, n)) }
        }
        for (status, n) in counts where !order.contains(status) {
            result.append((status, n))
        }
        return result
    }

    var body: some View {
        NavigationSplitView {
            List(filteredFiles, id: \.id, selection: $selectedFileId) { file in
                let isMissing = file.status == "missing"
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        statusDot(file.status)
                        if isMissing {
                            Image(systemName: "trash.slash.fill")
                                .foregroundStyle(.red)
                                .font(.caption)
                                .help("Source file no longer exists on disk")
                        }
                        Text(file.fileName)
                            .fontWeight(.medium)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .strikethrough(isMissing, color: .red)
                        if file.extensionMismatch {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .foregroundStyle(.orange)
                                .font(.caption)
                                .help("Extension does not match detected format: \(file.detectedMime ?? "unknown")")
                        }
                    }
                    HStack(spacing: 8) {
                        Label(formatBytes(file.sizeBytes), systemImage: "doc")
                        if let mime = file.mimeType {
                            Text(mime)
                        }
                        Text(file.status)
                            .foregroundStyle(statusColor(file.status))
                            .fontWeight(isMissing ? .semibold : .regular)
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .labelStyle(.titleOnly)
                }
                .padding(.vertical, 2)
                .opacity(isMissing ? 0.6 : 1.0)
            }
            .safeAreaInset(edge: .top, spacing: 0) {
                statusSummaryBar
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
        // Auto-refresh whenever the extraction pipeline reports activity.
        // extractionsInFlight changes on enqueue AND completion, so this
        // catches both "work started" and "work finished" transitions.
        .onChange(of: ingestion.extractionsInFlight) { _, _ in reload() }
        .onChange(of: ingestion.extractionsCompleted) { _, _ in reload() }
        .onChange(of: ingestion.extractionsFailed) { _, _ in reload() }
        .onChange(of: ingestion.totalIngested) { _, _ in reload() }
        .toolbar {
            ToolbarItem {
                Button(action: reload) {
                    Image(systemName: "arrow.clockwise")
                }
                .help("Refresh")
            }
            ToolbarItem {
                Button(action: reextractSelected) {
                    Image(systemName: "arrow.triangle.2.circlepath")
                }
                .disabled(selectedFile == nil)
                .help("Re-extract content for selected file")
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

    private var statusSummaryBar: some View {
        HStack(spacing: 8) {
            if statusCounts.isEmpty {
                Text("No files")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(statusCounts, id: \.0) { (status, count) in
                    HStack(spacing: 4) {
                        Circle()
                            .fill(statusColor(status))
                            .frame(width: 6, height: 6)
                        Text("\(count)")
                            .monospacedDigit()
                        Text(statusLabel(status))
                            .foregroundStyle(.secondary)
                    }
                    .font(.caption)
                }
            }
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.bar)
    }

    private func statusLabel(_ status: String) -> String {
        switch status {
        case "indexed": "indexed"
        case "pending": "pending"
        case "extracting": "extracting"
        case "unsupported": "unsupported"
        case "extract_failed": "failed"
        case "missing": "missing"
        default: status
        }
    }

    private func statusDot(_ status: String) -> some View {
        Circle()
            .fill(statusColor(status))
            .frame(width: 7, height: 7)
    }

    private func statusColor(_ status: String) -> Color {
        switch status {
        case "indexed": .green
        case "pending", "extracting": .orange
        case "missing": .red
        case "unsupported": .secondary
        case "extract_failed": .red
        default: .gray
        }
    }

    private func formatBytes(_ bytes: Int64) -> String {
        let formatter = ByteCountFormatter()
        formatter.countStyle = .file
        return formatter.string(fromByteCount: bytes)
    }

    private func reextractSelected() {
        guard let id = selectedFileId else { return }
        HollowBridge.shared.markForReextraction(fileId: id)
        // Extract synchronously so the result is visible immediately on reload.
        // Heavy work; consider moving to a background Task if files are large.
        _ = HollowBridge.shared.extractContent(fileId: id)
        reload()
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

            section("Detection") {
                row("Detected MIME", file.detectedMime ?? "—")
                row("Mismatch", file.extensionMismatch ? "⚠ yes" : "no")
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
