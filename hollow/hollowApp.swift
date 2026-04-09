import SwiftUI
import os

@main
struct hollowApp: App {
    @State private var ingestionService = IngestionService()
    @AppStorage("debugMode") private var debugMode = false
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(ingestionService)
                .onAppear {
                    ingestionService.start()
                    RustLogRelay.shared.start()
                    HollowLogger.app.info("hollow app launched")
                }
        }
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
}
