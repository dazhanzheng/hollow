import SwiftUI
import OSLog

struct LogViewerView: View {
    @State private var entries: [OSLogEntryLog] = []
    @State private var selectedTab = 0       // 0=Swift, 1=Rust
    @State private var filterLevel: OSLogEntryLog.Level? = nil
    @State private var searchText = ""
    @State private var autoRefresh = true
    @State private var refreshTimer: Timer?

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

                Button(action: refresh) {
                    Image(systemName: "arrow.clockwise")
                }
                .help("Refresh")

                Text("\(filteredEntries.count) entries")
                    .foregroundStyle(.secondary)
                    .font(.caption)
            }
            .padding(8)

            Divider()

            // Log list
            ScrollViewReader { proxy in
                List(filteredEntries, id: \.self) { entry in
                    HStack(alignment: .top, spacing: 8) {
                        Text(formatTime(entry.date))
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .frame(width: 80, alignment: .leading)

                        levelBadge(entry.level)
                            .frame(width: 50)

                        Text(entry.category)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .frame(width: 80, alignment: .leading)

                        Text(entry.composedMessage)
                            .font(.system(.caption, design: .monospaced))
                            .lineLimit(nil)
                            .textSelection(.enabled)
                    }
                    .id(entry)
                }
                .onChange(of: filteredEntries.count) {
                    if autoRefresh, let last = filteredEntries.last {
                        proxy.scrollTo(last, anchor: .bottom)
                    }
                }
            }
        }
        .frame(minWidth: 700, minHeight: 400)
        .onAppear {
            refresh()
            startAutoRefresh()
        }
        .onDisappear {
            stopAutoRefresh()
        }
        .onChange(of: autoRefresh) {
            if autoRefresh { startAutoRefresh() } else { stopAutoRefresh() }
        }
    }

    private var filteredEntries: [OSLogEntryLog] {
        var result = entries

        // Tab filter: Swift (non-RustCore) vs Rust (RustCore only)
        if selectedTab == 0 {
            result = result.filter { $0.category != rustCategory }
        } else {
            result = result.filter { $0.category == rustCategory }
        }

        // Level filter
        if let level = filterLevel {
            result = result.filter { $0.level == level }
        }

        // Search filter
        if !searchText.isEmpty {
            let query = searchText.lowercased()
            result = result.filter {
                $0.composedMessage.lowercased().contains(query) ||
                $0.category.lowercased().contains(query)
            }
        }

        return result
    }

    private func refresh() {
        do {
            let store = try OSLogStore(scope: .currentProcessIdentifier)
            let position = store.position(date: Date().addingTimeInterval(-3600))
            let predicate = NSPredicate(format: "subsystem == %@", subsystem)
            let rawEntries = try store.getEntries(at: position, matching: predicate)
            entries = rawEntries.compactMap { $0 as? OSLogEntryLog }
        } catch {
            HollowLogger.app.error("Failed to read OSLogStore: \(error)")
        }
    }

    private func startAutoRefresh() {
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 2.0, repeats: true) { _ in
            refresh()
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
