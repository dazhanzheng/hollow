import SwiftUI

struct ContentView: View {
    @Environment(IngestionService.self) private var ingestion

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "archivebox")
                .imageScale(.large)
                .foregroundStyle(.tint)
            Text("hollow")
                .font(.title)

            HStack(spacing: 6) {
                Circle()
                    .fill(ingestion.isWatching ? .green : .gray)
                    .frame(width: 8, height: 8)
                Text(ingestion.isWatching ? "Watching ~/Hollow Inbox/" : "Not watching")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }

            Text("\(ingestion.totalIngested) files ingested")
                .font(.headline)

            if let progress = ingestion.processingProgress {
                Text(progress)
                    .font(.caption)
                    .foregroundStyle(.orange)
            }

            if !ingestion.recentFiles.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Recent:")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    ForEach(ingestion.recentFiles, id: \.self) { name in
                        Text(name)
                            .font(.caption)
                            .lineLimit(1)
                    }
                }
                .frame(maxWidth: 300, alignment: .leading)
            }

            if let error = ingestion.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
        .padding()
        .frame(minWidth: 350, minHeight: 300)
    }
}
