import SwiftUI
import OSLog

private struct LogRow: Identifiable, Equatable {
    let id: Int
    let date: Date
    let level: OSLogEntryLog.Level
    let category: String
    let message: String
}

struct LogViewerView: View {
    @State private var rows: [LogRow] = []
    @State private var selectedTab = 0
    @State private var filterLevel: OSLogEntryLog.Level? = nil
    @State private var searchText = ""
    @State private var autoRefresh = true
    @State private var refreshTimer: Timer?
    @State private var isLoading = false

    private let subsystem = "com.syncpulse.hollow"
    private let rustCategory = "RustCore"

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            logList
        }
        .frame(minWidth: 720, minHeight: 420)
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

    // MARK: - Toolbar

    private var toolbar: some View {
        HStack(spacing: 12) {
            Picker("", selection: $selectedTab) {
                Text("Swift").tag(0)
                Text("Rust").tag(1)
            }
            .pickerStyle(.segmented)
            .frame(width: 150)

            Picker("Level", selection: $filterLevel) {
                Text("All Levels").tag(nil as OSLogEntryLog.Level?)
                Divider()
                Text("Debug").tag(OSLogEntryLog.Level.debug as OSLogEntryLog.Level?)
                Text("Info").tag(OSLogEntryLog.Level.info as OSLogEntryLog.Level?)
                Text("Warning").tag(OSLogEntryLog.Level.notice as OSLogEntryLog.Level?)
                Text("Error").tag(OSLogEntryLog.Level.error as OSLogEntryLog.Level?)
            }
            .frame(width: 130)

            TextField("Search...", text: $searchText)
                .textFieldStyle(.roundedBorder)
                .frame(maxWidth: 200)

            Spacer()

            Toggle("Auto", isOn: $autoRefresh)
                .toggleStyle(.switch)
                .controlSize(.small)

            Button(action: refreshAsync) {
                Image(systemName: "arrow.clockwise")
            }
            .disabled(isLoading)
            .help("Refresh")

            if isLoading {
                ProgressView()
                    .controlSize(.small)
            }

            Text("\(filteredRows.count)")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(.quaternary, in: Capsule())
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    // MARK: - Log List

    private var logList: some View {
        ScrollViewReader { proxy in
            List(filteredRows) { row in
                HStack(alignment: .top, spacing: 8) {
                    Text(formatTime(row.date))
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(.tertiary)
                        .frame(width: 72, alignment: .leading)

                    levelBadge(row.level)

                    Text(row.category)
                        .font(.caption.weight(.medium))
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

    // MARK: - Filtering

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

    // MARK: - Data Fetching

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

    // MARK: - Helpers

    private func formatTime(_ date: Date) -> String {
        let fmt = DateFormatter()
        fmt.dateFormat = "HH:mm:ss"
        return fmt.string(from: date)
    }

    @ViewBuilder
    private func levelBadge(_ level: OSLogEntryLog.Level) -> some View {
        let (text, color): (String, Color) = switch level {
        case .debug: ("DBG", .gray)
        case .info: ("INF", .blue)
        case .notice: ("WRN", .orange)
        case .error: ("ERR", .red)
        case .fault: ("FLT", .red)
        default: ("???", .gray)
        }
        Text(text)
            .font(.system(.caption2, design: .monospaced, weight: .bold))
            .foregroundStyle(color)
            .frame(width: 32)
    }
}
