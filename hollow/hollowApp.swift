import SwiftUI
import os

@main
struct hollowApp: App {
    @State private var ingestionService = IngestionService()
    @State private var showSidebarPrompt = false
    @AppStorage("debugMode") private var debugMode = false
    @AppStorage("appLanguage") private var appLanguage = ""
    @AppStorage("sidebarPromptDismissed") private var sidebarPromptDismissed = false
    @Environment(\.openWindow) private var openWindow

    init() {
        // Apply language override before any UI renders
        let lang = UserDefaults.standard.string(forKey: "appLanguage") ?? ""
        if !lang.isEmpty {
            UserDefaults.standard.set([lang], forKey: "AppleLanguages")
        } else {
            UserDefaults.standard.removeObject(forKey: "AppleLanguages")
        }
    }

    private var activeLocale: Locale {
        appLanguage.isEmpty ? .autoupdatingCurrent : Locale(identifier: appLanguage)
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(ingestionService)
                .environment(\.locale, activeLocale)
                .onAppear {
                    ingestionService.start()
                    RustLogRelay.shared.start()
                    HollowLogger.app.info("hollow app launched")
                    promptSidebarIfNeeded()
                }
                .sheet(isPresented: $showSidebarPrompt) {
                    SidebarPromptView(isPresented: $showSidebarPrompt) {
                        sidebarPromptDismissed = true
                    }
                }
        }
        .windowResizability(.contentSize)
        .commands {
            if debugMode {
                CommandMenu("Debug") {
                    Button("Database Browser") {
                        openWindow(id: "database-browser")
                    }
                    .keyboardShortcut("D", modifiers: [.command, .shift])

                    Button("Log Viewer") {
                        openWindow(id: "log-viewer")
                    }
                    .keyboardShortcut("L", modifiers: [.command, .shift])

                    Divider()

                    Button("Delete Database and Quit") {
                        confirmAndDeleteDatabase()
                    }
                }
            }
        }

        Settings {
            SettingsView()
                .environment(\.locale, activeLocale)
        }

        Window("Database Browser", id: "database-browser") {
            DatabaseBrowserView()
                .environment(\.locale, activeLocale)
        }

        Window("Log Viewer", id: "log-viewer") {
            LogViewerView()
                .environment(\.locale, activeLocale)
        }
    }

    private func confirmAndDeleteDatabase() {
        let alert = NSAlert()
        alert.messageText = String(localized: "Delete database and quit?")
        alert.informativeText = String(localized: "This permanently deletes hollow.db and all file records. Your files in ~/Hollow Inbox/ are not touched. Hollow will quit immediately; a fresh database is created on next launch.")
        alert.alertStyle = .warning
        alert.addButton(withTitle: String(localized: "Delete and Quit"))
        alert.addButton(withTitle: String(localized: "Cancel"))

        guard alert.runModal() == .alertFirstButtonReturn else { return }

        do {
            let dbPath = try HollowBridge.databasePath()
            let fm = FileManager.default

            // Stop the ingestion service so nothing writes during / after deletion.
            ingestionService.stop()

            // Remove hollow.db plus any SQLite sidecar files (-wal, -shm).
            for path in [dbPath, dbPath + "-wal", dbPath + "-shm"] {
                if fm.fileExists(atPath: path) {
                    try fm.removeItem(atPath: path)
                    HollowLogger.app.info("Deleted \(path)")
                }
            }
        } catch {
            HollowLogger.app.error("Failed to delete database: \(error.localizedDescription)")
        }

        NSApplication.shared.terminate(nil)
    }

    private func promptSidebarIfNeeded() {
        guard !sidebarPromptDismissed else { return }

        let inboxURL = FileWatcher.inboxURL
        let parentURL = inboxURL.deletingLastPathComponent()

        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
            NSWorkspace.shared.selectFile(
                inboxURL.path,
                inFileViewerRootedAtPath: parentURL.path
            )
            showSidebarPrompt = true
            HollowLogger.app.info("Showing sidebar prompt, revealed Hollow Inbox in Finder")
        }
    }
}

// MARK: - Sidebar Prompt

private struct SidebarPromptView: View {
    @Binding var isPresented: Bool
    let onDismiss: () -> Void

    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "sidebar.leading")
                .font(.system(size: 40))
                .foregroundStyle(.tint)

            Text("Add Hollow Inbox to Finder Sidebar")
                .font(.title3.weight(.semibold))

            VStack(alignment: .leading, spacing: 8) {
                instructionRow(step: "1", text: String(localized: "Finder has opened your home folder"))
                instructionRow(step: "2", text: String(localized: "Drag the \"Hollow Inbox\" folder into Finder's sidebar under Favorites"))
            }
            .padding(.horizontal)

            Text("This makes it easy to drop files into Hollow at any time.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            HStack(spacing: 12) {
                Button("Remind Me Next Time") {
                    isPresented = false
                }

                Button("Done, I've Added It") {
                    onDismiss()
                    isPresented = false
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(28)
        .frame(width: 400)
    }

    private func instructionRow(step: String, text: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Text(step)
                .font(.callout.weight(.bold))
                .foregroundStyle(.white)
                .frame(width: 24, height: 24)
                .background(.tint, in: Circle())
            Text(text)
                .font(.callout)
        }
    }
}
