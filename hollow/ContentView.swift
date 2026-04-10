import SwiftUI

struct ContentView: View {
    @Environment(IngestionService.self) private var ingestion

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
            Text("\(ingestion.totalIngested) files ingested")
                .font(.title2.weight(.medium).monospacedDigit())

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

            Spacer()
        }
        .padding(24)
        .frame(minWidth: 380, minHeight: 340)
    }
}
