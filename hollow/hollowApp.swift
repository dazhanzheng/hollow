import SwiftUI

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
                }
        }
        .commands {
            if debugMode {
                CommandMenu("Debug") {
                    Button("Database Browser") {
                        openWindow(id: "database-browser")
                    }
                    .keyboardShortcut("D", modifiers: [.command, .shift])
                }
            }
        }

        Settings {
            SettingsView()
        }

        Window("Database Browser", id: "database-browser") {
            DatabaseBrowserView()
        }
    }
}
