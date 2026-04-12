import SwiftUI

struct ContentView: View {
    @Environment(IngestionService.self) private var ingestion
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(spacing: 20) {
            Spacer()

            // App icon + title
            VStack(spacing: 8) {
                Image(systemName: "archivebox.fill")
                    .font(.system(size: 48))
                    .foregroundStyle(.tint)
                Text("Hollow")
                    .font(.largeTitle.weight(.semibold))
            }

            // Status pill
            HStack(spacing: 6) {
                Circle()
                    .fill(ingestion.isWatching ? .green : .gray)
                    .frame(width: 8, height: 8)
                Text(ingestion.isWatching ? "Watching ~/Hollow Inbox/" : "Not watching")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(.quaternary, in: Capsule())

            // Stats
            VStack(spacing: 6) {
                Text("\(ingestion.totalIngested) files tracked")
                    .font(.title2.weight(.medium).monospacedDigit())

                HStack(spacing: 14) {
                    if ingestion.extractionsInFlight > 0 {
                        Label("\(ingestion.extractionsInFlight) extracting", systemImage: "gearshape.2.fill")
                            .foregroundStyle(.orange)
                    }
                    Label("\(ingestion.extractionsCompleted) extracted", systemImage: "checkmark.seal.fill")
                        .foregroundStyle(.green)
                    if ingestion.extractionsFailed > 0 {
                        Label("\(ingestion.extractionsFailed) failed", systemImage: "exclamationmark.triangle.fill")
                            .foregroundStyle(.red)
                    }
                }
                .font(.caption.monospacedDigit())
                .labelStyle(.titleAndIcon)
            }

            // Embedding status
            EmbeddingStatusBadge()

            // Search button
            Button {
                openWindow(id: "search")
            } label: {
                Label("Search Files", systemImage: "magnifyingglass")
            }
            .buttonStyle(.borderedProminent)

            // Recent files
            if !ingestion.recentFiles.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Label("Recent", systemImage: "clock")
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(.secondary)

                    ForEach(ingestion.recentFiles, id: \.self) { name in
                        HStack(spacing: 6) {
                            Image(systemName: "doc")
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                            Text(name)
                                .font(.callout)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }
                    }
                }
                .frame(maxWidth: 280, alignment: .leading)
                .padding()
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
            }

            // Error banner
            if let error = ingestion.lastError {
                Label(error, systemImage: "exclamationmark.triangle.fill")
                    .font(.callout)
                    .foregroundStyle(.white)
                    .padding(10)
                    .background(.red.gradient, in: RoundedRectangle(cornerRadius: 8))
            }

            // Extraction error banner
            if let extractionError = ingestion.lastExtractionError {
                Label("Extraction: \(extractionError)", systemImage: "doc.text.magnifyingglass")
                    .font(.callout)
                    .foregroundStyle(.white)
                    .padding(10)
                    .background(.orange.gradient, in: RoundedRectangle(cornerRadius: 8))
            }

            Spacer()
        }
        .padding(24)
        .frame(minWidth: 380, minHeight: 340)
    }
}

// MARK: - Embedding Status Badge

/// Self-contained badge that polls embedding readiness and stats.
/// Shows one of three states: not installed, ready (with counts), or processing.
private struct EmbeddingStatusBadge: View {
    @State private var modelReady = false
    @State private var onnxReady = false
    @State private var status: EmbeddingStatus?
    @State private var refreshTimer: Timer?

    var body: some View {
        Group {
            if !modelReady {
                // Model not downloaded
                HStack(spacing: 6) {
                    Image(systemName: "brain")
                        .foregroundStyle(.secondary)
                    Text("Semantic search: model not downloaded")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            } else if !onnxReady {
                // Model downloaded but ONNX Runtime missing
                HStack(spacing: 6) {
                    Image(systemName: "exclamationmark.triangle")
                        .foregroundStyle(.orange)
                    Text("Semantic search: runtime missing")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            } else if let status {
                // Everything ready — show stats
                HStack(spacing: 6) {
                    Image(systemName: "brain")
                        .foregroundStyle(.green)
                    if status.pendingEmbedding > 0 {
                        Text("\(status.totalEmbedded)/\(status.totalIndexed) embedded")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.orange)
                    } else {
                        Text("\(status.totalEmbedded) embedded")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.green)
                    }
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
        .background(.quaternary, in: Capsule())
        .task {
            refresh()
        }
        .onAppear {
            // Poll every 5s to catch model downloads and embedding progress
            refreshTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { _ in
                Task { @MainActor in refresh() }
            }
        }
        .onDisappear {
            refreshTimer?.invalidate()
        }
    }

    private func refresh() {
        modelReady = HollowBridge.shared.isEmbeddingReady()
        if let modelsDir = try? HollowBridge.modelsDirectory() {
            onnxReady = FileManager.default.fileExists(
                atPath: modelsDir.appendingPathComponent("libonnxruntime.dylib").path
            )
        }
        status = HollowBridge.shared.getEmbeddingStatus()
    }
}
