import SwiftUI
import ServiceManagement

struct SettingsView: View {
    var body: some View {
        TabView {
            GeneralSettingsView()
                .tabItem {
                    Label("General", systemImage: "gear")
                }

            PluginsSettingsView()
                .tabItem {
                    Label("Plugins", systemImage: "puzzlepiece.extension")
                }

            ModelsSettingsView()
                .tabItem {
                    Label("Models", systemImage: "cpu")
                }

            AdvancedSettingsView()
                .tabItem {
                    Label("Advanced", systemImage: "slider.horizontal.3")
                }

            DeveloperSettingsView()
                .tabItem {
                    Label("Developer", systemImage: "hammer")
                }
        }
        .frame(width: 560, height: 460)
    }
}

// MARK: - General

private struct GeneralSettingsView: View {
    @AppStorage("appLanguage") private var appLanguage = ""
    @AppStorage("showMenuBarIcon") private var showMenuBarIcon = true
    /// Mirrors the real `SMAppService.mainApp.status`. We don't persist this
    /// in UserDefaults — it's derived from system state and refreshed via
    /// `.task` whenever the pane appears.
    @State private var launchAtLogin: Bool = LaunchAtLogin.isEnabled
    @State private var launchAtLoginNeedsApproval: Bool = false

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
        generalForm
            .task {
                launchAtLogin = LaunchAtLogin.isEnabled
                launchAtLoginNeedsApproval = (LaunchAtLogin.status == .requiresApproval)
            }
    }

    private var generalForm: some View {
        Form {
            Section("Startup") {
                Toggle("Launch Hollow at login", isOn: Binding(
                    get: { launchAtLogin },
                    set: { newValue in
                        LaunchAtLogin.setEnabled(newValue)
                        // Re-read system state — the toggle reflects real state,
                        // not just what we asked for (user may need to approve
                        // in System Settings, or the request may fail outright).
                        launchAtLogin = LaunchAtLogin.isEnabled
                        launchAtLoginNeedsApproval = (LaunchAtLogin.status == .requiresApproval)
                    }
                ))

                if launchAtLoginNeedsApproval {
                    HStack(spacing: 6) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(.orange)
                        Text("Approval required — open System Settings → Login Items.")
                            .font(.caption)
                        Button("Open") {
                            LaunchAtLogin.openSystemSettingsLoginItems()
                        }
                        .buttonStyle(.link)
                    }
                } else {
                    Text("When enabled, Hollow starts automatically when you log in and runs quietly in the menu bar.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            Section("Menu Bar") {
                Toggle("Show Hollow in menu bar", isOn: $showMenuBarIcon)
                Text("Keeps a status icon in the menu bar with quick access to stats, inbox, and controls.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

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
                            .lineLimit(1)
                            .truncationMode(.middle)
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
                            .lineLimit(1)
                            .truncationMode(.middle)
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
        }
        .formStyle(.grouped)
    }
}

// MARK: - Plugins

/// Unified display model for the Settings Plugins tab. Abstracts over the
/// Rust-side `ExtractorPluginInfo` (FFI record) and Swift-side
/// `SwiftExtractor` instances so the UI can render both in one list.
private struct PluginDisplayInfo: Identifiable {
    let id: String
    let name: String
    let displayName: String
    let description: String
    let extensions: [String]
    /// True if the backing implementation lives in Swift (Apple Vision,
    /// PDFKit, etc.). Toggling a Swift plugin only touches UserDefaults —
    /// Rust plugins additionally push the change down into the Rust
    /// pipeline's disabled-set.
    let isSwift: Bool
}

private struct PluginsSettingsView: View {
    @State private var plugins: [PluginDisplayInfo] = []

    var body: some View {
        Form {
            Section {
                if plugins.isEmpty {
                    Text("No parser plugins available.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(plugins) { info in
                        PluginToggleRow(info: info)
                    }
                }
            } header: {
                Text("Parser Plugins")
            } footer: {
                Text("Disabled plugins are skipped during content extraction. Affected files will be marked as unsupported until you re-enable the plugin and re-extract them.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .task {
            plugins = Self.loadAllPlugins()
        }
    }

    /// Merge Rust and Swift plugin lists into one flat array. Rust plugins
    /// come first (historical order), Swift plugins (Apple Vision) after.
    private static func loadAllPlugins() -> [PluginDisplayInfo] {
        let rust = HollowBridge.shared.listExtractors().map { info in
            PluginDisplayInfo(
                id: info.name,
                name: info.name,
                displayName: info.displayName,
                description: info.description,
                extensions: info.extensions,
                isSwift: false
            )
        }
        let swift = SwiftExtractorRegistry.shared.all.map { extractor in
            PluginDisplayInfo(
                id: extractor.name,
                name: extractor.name,
                displayName: extractor.displayName,
                description: extractor.description,
                extensions: extractor.supportedExtensions,
                isSwift: true
            )
        }
        return rust + swift
    }
}

private struct PluginToggleRow: View {
    let info: PluginDisplayInfo
    @State private var isEnabled: Bool = true

    var body: some View {
        Toggle(isOn: Binding(
            get: { isEnabled },
            set: { newValue in
                isEnabled = newValue
                // Persist to the shared UserDefaults key format. For Rust
                // plugins we also push to the Rust pipeline immediately
                // so new extractions respect the change; Swift plugins
                // re-read UserDefaults on every lookup so no push needed.
                UserDefaults.standard.set(
                    newValue,
                    forKey: "plugin.enabled.\(info.name)"
                )
                if !info.isSwift {
                    HollowBridge.shared.setExtractorEnabled(
                        name: info.name,
                        enabled: newValue
                    )
                }
            }
        )) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(info.displayName)
                        .font(.body)
                    if info.isSwift {
                        Text("LOCAL")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(.tint, in: Capsule())
                    }
                }
                Text(info.description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                if !info.extensions.isEmpty {
                    Text(info.extensions.prefix(12).map { ".\($0)" }.joined(separator: " ")
                         + (info.extensions.count > 12 ? " …" : ""))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .monospaced()
                }
            }
        }
        .task {
            let key = "plugin.enabled.\(info.name)"
            isEnabled = UserDefaults.standard.object(forKey: key) as? Bool ?? true
        }
    }
}

// MARK: - Models

private struct ModelsSettingsView: View {
    @State private var models: [EmbeddingModelInfo] = []
    @State private var embeddingStatus: EmbeddingStatus?
    @State private var refreshID = UUID()

    var body: some View {
        Form {
            Section {
                ForEach(models, id: \.name) { model in
                    ModelRow(model: model, onDownloadComplete: {
                        refreshID = UUID()
                    })
                }
            } header: {
                Text("Embedding Models")
            } footer: {
                Text("Embedding models enable semantic search — finding files by meaning, not just keywords. Models run locally on your Mac.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section("Runtime") {
                let onnxReady = (try? HollowBridge.modelsDirectory())
                    .map { FileManager.default.fileExists(atPath: $0.appendingPathComponent("libonnxruntime.dylib").path) } ?? false

                LabeledContent("ONNX Runtime") {
                    if onnxReady {
                        Label("Installed", systemImage: "checkmark.circle.fill")
                            .foregroundStyle(.green)
                            .font(.caption)
                    } else {
                        Label("Not installed", systemImage: "xmark.circle")
                            .foregroundStyle(.secondary)
                            .font(.caption)
                    }
                }
                Text("Downloaded automatically with the first model.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            if let status = embeddingStatus {
                Section("Embedding Status") {
                    LabeledContent("Files indexed") {
                        Text("\(status.totalIndexed)")
                            .monospacedDigit()
                    }
                    LabeledContent("Files embedded") {
                        Text("\(status.totalEmbedded)")
                            .monospacedDigit()
                    }
                    if status.pendingEmbedding > 0 {
                        LabeledContent("Pending") {
                            Text("\(status.pendingEmbedding)")
                                .monospacedDigit()
                                .foregroundStyle(.orange)
                        }
                        Button("Embed All Now") {
                            NotificationCenter.default.post(name: .fileIndexed, object: nil)
                            // Refresh after a delay to show updated counts
                            DispatchQueue.main.asyncAfter(deadline: .now() + 5) {
                                refreshID = UUID()
                            }
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .task(id: refreshID) {
            models = HollowBridge.shared.listEmbeddingModels()
            embeddingStatus = HollowBridge.shared.getEmbeddingStatus()
        }
    }
}

private struct ModelRow: View {
    let model: EmbeddingModelInfo
    var onDownloadComplete: (() -> Void)?
    @State private var downloader = ModelDownloader()

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 4) {
                Text(model.displayName)
                    .font(.body.weight(.medium))
                Text(model.description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                HStack(spacing: 8) {
                    Text("Download: \(model.downloadSizeMb) MB")
                    Text("RAM: ~\(model.ramUsageMb) MB")
                }
                .font(.caption2)
                .foregroundStyle(.tertiary)

                // RAM warning for large models
                if model.ramUsageMb >= 3000 && !model.isDownloaded {
                    let ram = ProcessInfo.processInfo.physicalMemory / (1024 * 1024 * 1024)
                    if ram < 32 {
                        HStack(spacing: 4) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .foregroundStyle(.orange)
                            Text("Your Mac has \(ram) GB RAM. This model may slow down other apps.")
                                .font(.caption2)
                                .foregroundStyle(.orange)
                        }
                    }
                }

                if let error = downloader.error {
                    Text(error)
                        .font(.caption2)
                        .foregroundStyle(.red)
                }
            }

            Spacer()

            if model.isDownloaded {
                VStack(spacing: 4) {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                    Button("Delete", role: .destructive) {
                        deleteModel()
                    }
                    .font(.caption2)
                    .buttonStyle(.plain)
                    .foregroundStyle(.red)
                }
            } else if model.description.contains("Coming soon") {
                Text("Coming soon")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else if downloader.isDownloading {
                VStack(spacing: 4) {
                    ProgressView(value: downloader.progress)
                        .frame(width: 80)
                    Text("\(Int(downloader.progress * 100))%")
                        .font(.caption2)
                        .monospacedDigit()
                }
            } else {
                Button("Download") {
                    Task {
                        try? await downloader.downloadDefaultModel()
                        if downloader.error == nil {
                            onDownloadComplete?()
                        }
                    }
                }
                .buttonStyle(.bordered)
            }
        }
    }

    private func deleteModel() {
        guard let modelsDir = try? HollowBridge.modelsDirectory() else { return }
        let modelDir = modelsDir.appendingPathComponent(model.name)
        try? FileManager.default.removeItem(at: modelDir)
        // Also remove ONNX Runtime dylib since it's only needed for embedding
        let dylib = modelsDir.appendingPathComponent("libonnxruntime.dylib")
        try? FileManager.default.removeItem(at: dylib)
        onDownloadComplete?() // triggers refresh
    }
}

// MARK: - Advanced

private struct AdvancedSettingsView: View {
    @AppStorage("enableFullHash") private var enableFullHash = false
    @State private var isComputingFullHash = false
    @State private var fullHashProgress: String?

    var body: some View {
        Form {
            Section("Performance") {
                LabeledContent("Extraction workers") {
                    Text("\(IngestionService.workerConcurrency)")
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
                Text("Number of parallel workers used for metadata intake and content extraction. Derived from CPU core count.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
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
        }
        .formStyle(.grouped)
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

// MARK: - Developer

private struct DeveloperSettingsView: View {
    @AppStorage("debugMode") private var debugMode = false

    var body: some View {
        Form {
            Section("Developer") {
                Toggle("Debug Mode", isOn: $debugMode)
                Text("Shows Debug menu in the menu bar with database browser and log viewer.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }
}
