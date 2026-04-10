import SwiftUI
import os

@main
struct hollowApp: App {
    @State private var ingestionService = IngestionService()
    @State private var showSidebarPrompt = false
    @AppStorage("debugMode") private var debugMode = false
    @AppStorage("sidebarPromptDismissed") private var sidebarPromptDismissed = false
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(ingestionService)
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

    private func promptSidebarIfNeeded() {
        guard !sidebarPromptDismissed else { return }

        let inboxURL = FileWatcher.inboxURL
        let parentURL = inboxURL.deletingLastPathComponent()

        // Open the parent folder in Finder with Hollow Inbox selected
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
                instructionRow(step: "1", text: "Finder has opened your home folder")
                instructionRow(step: "2", text: "Drag the \"Hollow Inbox\" folder into Finder's sidebar under Favorites")
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
