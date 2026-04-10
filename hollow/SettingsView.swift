import SwiftUI

struct SettingsView: View {
    @AppStorage("enableFullHash") private var enableFullHash = false
    @AppStorage("debugMode") private var debugMode = false
    @AppStorage("appLanguage") private var appLanguage = ""
    @State private var isComputingFullHash = false
    @State private var fullHashProgress: String?

    private let inboxPath = FileWatcher.inboxURL.path
    private let dbPath: String = {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!
        return appSupport
            .appendingPathComponent("com.syncpulse.hollow/hollow.db")
            .path
    }()

    var body: some View {
        Form {
            Section("Language") {
                Picker("Language", selection: $appLanguage) {
                    Text("System Default").tag("")
                    Divider()
                    Text("English").tag("en")
                    Text("简体中文").tag("zh-Hans")
                }
                Text("Restart the app after changing language.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Storage") {
                LabeledContent("Inbox Folder") {
                    HStack(spacing: 4) {
                        Text(inboxPath)
                            .textSelection(.enabled)
                            .foregroundStyle(.secondary)
                        Button {
                            NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: inboxPath)
                        } label: {
                            Image(systemName: "folder")
                        }
                        .buttonStyle(.borderless)
                        .help("Reveal in Finder")
                    }
                }
                LabeledContent("Database") {
                    HStack(spacing: 4) {
                        Text(dbPath)
                            .textSelection(.enabled)
                            .foregroundStyle(.secondary)
                        Button {
                            NSWorkspace.shared.activateFileViewerSelecting(
                                [URL(fileURLWithPath: dbPath)]
                            )
                        } label: {
                            Image(systemName: "folder")
                        }
                        .buttonStyle(.borderless)
                        .help("Reveal in Finder")
                    }
                }
            }

            Section("Hashing") {
                Toggle("Compute full SHA-256 hash for all files", isOn: $enableFullHash)

                Text("Quick hash (sampled) is always computed at intake. Full hash reads the entire file and is useful for 100% accurate duplicate detection.")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                if enableFullHash {
                    HStack(spacing: 8) {
                        Button("Run Full Hash Now") {
                            runFullHashForAll()
                        }
                        .disabled(isComputingFullHash)

                        if isComputingFullHash {
                            ProgressView()
                                .controlSize(.small)
                        }

                        if let progress = fullHashProgress {
                            Text(progress)
                                .font(.caption)
                                .foregroundStyle(.orange)
                                .monospacedDigit()
                        }
                    }
                }
            }

            Section("Performance") {
                LabeledContent("Extraction workers") {
                    Text("\(IngestionService.workerConcurrency)")
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
            }

            Section("Developer") {
                Toggle("Debug Mode", isOn: $debugMode)
                Text("Shows Debug menu in the menu bar with database browser and log viewer.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 480)
        .padding()
    }

    private func runFullHashForAll() {
        isComputingFullHash = true
        fullHashProgress = "Starting..."

        let bridge = HollowBridge.shared
        DispatchQueue.global(qos: .utility).async {
            let allFiles = bridge.listFiles(limit: UInt32.max, offset: 0)
            let needsHash = allFiles.filter { $0.hash.isEmpty }
            let total = needsHash.count

            for (index, file) in needsHash.enumerated() {
                _ = bridge.computeHash(fileId: file.id)
                DispatchQueue.main.async {
                    fullHashProgress = "Hashing \(index + 1)/\(total)..."
                }
            }

            DispatchQueue.main.async {
                fullHashProgress = nil
                isComputingFullHash = false
            }
        }
    }
}
