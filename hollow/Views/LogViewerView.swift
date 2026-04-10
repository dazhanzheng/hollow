import SwiftUI
import OSLog

/// Lightweight value type extracted from OSLogEntryLog so we don't hold
/// heavyweight system objects in the view's state.
private struct LogRow: Identifiable, Equatable {
    let id: Int
    let date: Date
    let level: OSLogEntryLog.Level
    let category: String
    let message: String
}

struct LogViewerView: View {
    @State private var rows: [LogRow] = []
    @State private var selectedTab = 0       // 0=Swift, 1=Rust
    @State private var filterLevel: OSLogEntryLog.Level? = nil
    @State private var searchText = ""
    @State private var autoRefresh = true
    @State private var refreshTimer: Timer?
    @State private var isLoading = false

    private let subsystem = "com.syncpulse.hollow"
    private let rustCategory = "RustCore"

    var body: some View {
        VStack(spacing: 0) {
            // Toolbar
            HStack {
                Picker("", selection: $selectedTab) {
                    Text("Swift").tag(0)
                    Text("Rust").tag(1)
                }
                .pickerStyle(.segmented)
                .frame(width: 160)

                Picker("Level", selection: $filterLevel) {
                    Text("All").tag(nil as OSLogEntryLog.Level?)
                    Text("Debug").tag(OSLogEntryLog.Level.debug as OSLogEntryLog.Level?)
                    Text("Info").tag(OSLogEntryLog.Level.info as OSLogEntryLog.Level?)
                    Text("Warning").tag(OSLogEntryLog.Level.notice as OSLogEntryLog.Level?)
                    Text("Error").tag(OSLogEntryLog.Level.error as OSLogEntryLog.Level?)
                }
                .frame(width: 120)

                TextField("Search...", text: $searchText)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 200)

                Spacer()

                Toggle("Auto-refresh", isOn: $autoRefresh)
                    .toggleStyle(.switch)

                Button(action: refreshAsync) {
                    Image(systemName: "arrow.clockwise")
                }
                .help("Refresh")
                .disabled(isLoading)

                if isLoading {
                    ProgressView()
                        .controlSize(.small)
                }

                Text("\(filteredRows.count) entries")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }
            .padding(8)

            Divider()

            // Log list
            ScrollViewReader { proxy in
                List(filteredRows) { row in
                    HStack(alignment: .top, spacing: 8) {
                        Text(formatTime(row.date))
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .frame(width: 80, alignment: .leading)

                        levelBadge(row.level)
                            .frame(width: 50)

                        Text(row.category)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .frame(width: 80, alignment: .leading)

                        Text(row.message)
                            .font(.system(.caption, design: .monospaced))
                            .lineLimit(nil)
                            .textSelection(.enabled)
                    }
                }
                .onChange(of: filteredRows.count) {
                    if autoRefresh, let last = filteredRows.last {
                        proxy.scrollTo(last.id, anchor: .bottom)
                    }
                }
            }
        }
        .frame(minWidth: 700, minHeight: 400)
        .onAppear {
            refreshAsync()
            startAutoRefresh()
        }
        .onDisappear {
            stopAutoRefresh()
        }
        .onChange(of: autoRefresh) {
            if autoRefresh { startAutoRefresh() } else { stopAutoRefresh() }
        }
    }

    private var filteredRows: [LogRow] {
        var result = rows

        if selectedTab == 0 {
            result = result.filter { $0.category != rustCategory }
        } else {
            result = result.filter { $0.category == rustCategory }
        }

        if let level = filterLevel {
            result = result.filter { $0.level == level }
        }

        if !searchText.isEmpty {
            let query = searchText.lowercased()
            result = result.filter {
                $0.message.lowercased().contains(query) ||
                $0.category.lowercased().contains(query)
            }
        }

        return result
    }

    /// Read OSLogStore off the main thread, then update state on main.
    private func refreshAsync() {
        guard !isLoading else { return }
        isLoading = true
        let sub = subsystem
        DispatchQueue.global(qos: .userInitiated).async {
            let fetched = Self.fetchLogs(subsystem: sub)
            DispatchQueue.main.async {
                rows = fetched
                isLoading = false
            }
        }
    }

    private static func fetchLogs(subsystem: String) -> [LogRow] {
        do {
            let store = try OSLogStore(scope: .currentProcessIdentifier)
            let position = store.position(date: Date().addingTimeInterval(-3600))
            let predicate = NSPredicate(format: "subsystem == %@", subsystem)
            let rawEntries = try store.getEntries(at: position, matching: predicate)
            var result: [LogRow] = []
            var idx = 0
            for entry in rawEntries {
                guard let log = entry as? OSLogEntryLog else { continue }
                result.append(LogRow(
                    id: idx,
                    date: log.date,
                    level: log.level,
                    category: log.category,
                    message: log.composedMessage
                ))
                idx += 1
            }
            return result
        } catch {
            return []
        }
    }

    private func startAutoRefresh() {
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 5.0, repeats: true) { _ in
            refreshAsync()
        }
    }

    private func stopAutoRefresh() {
        refreshTimer?.invalidate()
        refreshTimer = nil
    }

    private func formatTime(_ date: Date) -> String {
        let fmt = DateFormatter()
        fmt.dateFormat = "HH:mm:ss"
        return fmt.string(from: date)
    }

    @ViewBuilder
    private func levelBadge(_ level: OSLogEntryLog.Level) -> some View {
        let (text, color): (String, Color) = switch level {
        case .debug: ("DEBUG", .gray)
        case .info: ("INFO", .blue)
        case .notice: ("WARN", .orange)
        case .error: ("ERROR", .red)
        case .fault: ("FAULT", .red)
        default: ("OTHER", .gray)
        }
        Text(text)
            .font(.system(.caption2, design: .monospaced, weight: .bold))
            .foregroundStyle(color)
    }
}
