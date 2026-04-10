import SwiftUI
import os

@main
struct hollowApp: App {
    @State private var ingestionService = IngestionService()
    @AppStorage("debugMode") private var debugMode = false
    @AppStorage("hasLaunchedBefore") private var hasLaunchedBefore = false
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(ingestionService)
                .onAppear {
                    ingestionService.start()
                    RustLogRelay.shared.start()
                    HollowLogger.app.info("hollow app launched")
                    handleFirstLaunch()
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
                }
            }
        }

        Settings {
            SettingsView()
        }

        Window("Database Browser", id: "database-browser") {
            DatabaseBrowserView()
        }

        Window("Log Viewer", id: "log-viewer") {
            LogViewerView()
        }
    }

    private func handleFirstLaunch() {
        guard !hasLaunchedBefore else { return }
        hasLaunchedBefore = true

        let inboxURL = FileWatcher.inboxURL

        // Reveal the inbox folder in Finder so the user can drag it to sidebar
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
            NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: inboxURL.path)
            HollowLogger.app.info("First launch: revealed Hollow Inbox in Finder")
        }
    }
}
