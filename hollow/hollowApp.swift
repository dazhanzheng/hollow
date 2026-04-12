import SwiftUI
import os

@main
struct hollowApp: App {
    @State private var ingestionService = IngestionService()
    @State private var embeddingService = EmbeddingService()
    @State private var showSidebarPrompt = false
    @State private var showLaunchAtLoginPrompt = false
    /// Guard against `.onAppear` re-running the launch sequence every time
    /// the main window is closed and reopened (e.g. via the menu bar item).
    /// `.onAppear` fires on every show, but we only want the ingestion
    /// service / log relay / prompts to run exactly once per process.
    @State private var didStartup = false
    @AppStorage("debugMode") private var debugMode = false
    @AppStorage("appLanguage") private var appLanguage = ""
    @AppStorage("sidebarPromptDismissed") private var sidebarPromptDismissed = false
    @AppStorage("launchAtLoginPromptDismissed") private var launchAtLoginPromptDismissed = false
    @AppStorage("showMenuBarIcon") private var showMenuBarIcon = true
    @AppStorage("hasShownModelOnboarding") private var hasShownOnboarding = false
    @State private var showModelOnboarding = false
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
        // Single-instance main window. Using `Window` (not `WindowGroup`) so
        // that `openWindow(id: "main")` reuses the existing instance instead
        // of creating a duplicate every time the user hits the menu bar item.
        Window("Hollow", id: "main") {
            ContentView()
                .environment(ingestionService)
                .environment(\.locale, activeLocale)
                .onAppear {
                    // `.onAppear` fires every time the window becomes
                    // visible, including when the user closes the main
                    // window and reopens it from the menu bar. Guard so
                    // the process-level launch sequence runs exactly once.
                    guard !didStartup else { return }
                    didStartup = true

                    ingestionService.start()
                    embeddingService.startListening()
                    RustLogRelay.shared.start()
                    HollowLogger.app.info("hollow app launched")
                    promptSidebarIfNeeded()
                    promptLaunchAtLoginIfNeeded()
                    promptModelOnboardingIfNeeded()
                }
                .sheet(isPresented: $showModelOnboarding) {
                    OnboardingModelView()
                }
                .sheet(isPresented: $showSidebarPrompt) {
                    SidebarPromptView(isPresented: $showSidebarPrompt) {
                        sidebarPromptDismissed = true
                    }
                }
                .sheet(isPresented: $showLaunchAtLoginPrompt) {
                    LaunchAtLoginPromptView(isPresented: $showLaunchAtLoginPrompt) {
                        launchAtLoginPromptDismissed = true
                    }
                }
        }
        .windowResizability(.contentSize)
        .commands {
            if debugMode {
                CommandMenu("Debug") {
                    Button("Database Browser") {
                        surfaceWindow(
                            open: { openWindow(id: "database-browser") },
                            matches: isWindowWithSceneId("database-browser")
                        )
                    }
                    .keyboardShortcut("D", modifiers: [.command, .shift])

                    Button("Log Viewer") {
                        surfaceWindow(
                            open: { openWindow(id: "log-viewer") },
                            matches: isWindowWithSceneId("log-viewer")
                        )
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
                .environment(ingestionService)
                .environment(\.locale, activeLocale)
        }

        Window("Log Viewer", id: "log-viewer") {
            LogViewerView()
                .environment(\.locale, activeLocale)
        }

        Window("Search", id: "search") {
            SearchView()
        }
        .defaultSize(width: 600, height: 500)

        MenuBarExtra(
            "Hollow",
            systemImage: ingestionService.isWatching ? "archivebox.fill" : "archivebox",
            isInserted: $showMenuBarIcon
        ) {
            MenuBarView()
                .environment(ingestionService)
                .environment(\.locale, activeLocale)
        }
        .menuBarExtraStyle(.window)
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

    /// Show the launch-at-login prompt if the user hasn't dismissed it yet
    /// AND the app isn't already set up to launch at login. If it's already
    /// enabled (or the user previously opted out), do nothing silently.
    private func promptLaunchAtLoginIfNeeded() {
        guard !launchAtLoginPromptDismissed else { return }
        guard !LaunchAtLogin.isEnabled else {
            // Already registered — no need to bother the user, and mark as
            // handled so we don't re-ask on the next launch.
            launchAtLoginPromptDismissed = true
            return
        }

        // Delay slightly so the main window has a chance to draw first.
        // Also avoids racing the sidebar prompt — we want one sheet at a time.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.8) {
            // Re-check on the timer in case state changed in the meantime.
            guard !launchAtLoginPromptDismissed, !LaunchAtLogin.isEnabled else { return }
            // Don't stack on top of the sidebar prompt if it's still up.
            guard !showSidebarPrompt else { return }
            showLaunchAtLoginPrompt = true
            HollowLogger.app.info("Showing launch-at-login prompt")
        }
    }

    private func promptModelOnboardingIfNeeded() {
        guard !hasShownOnboarding else { return }
        // Delay to avoid stacking on top of sidebar/launch-at-login prompts.
        DispatchQueue.main.asyncAfter(deadline: .now() + 3.0) {
            guard !hasShownOnboarding else { return }
            // Don't stack on top of other prompts.
            guard !showSidebarPrompt, !showLaunchAtLoginPrompt else { return }
            showModelOnboarding = true
        }
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

// MARK: - Launch-at-login Prompt

private struct LaunchAtLoginPromptView: View {
    @Binding var isPresented: Bool
    let onDismiss: () -> Void

    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "power.circle.fill")
                .font(.system(size: 40))
                .foregroundStyle(.tint)

            Text("Launch Hollow at Login?")
                .font(.title3.weight(.semibold))

            Text("Hollow works best when it's always running in the background — new files in your Inbox get picked up immediately, and the menu bar icon is always available.\n\nYou can change this anytime in Settings → General.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)

            HStack(spacing: 12) {
                Button("Not Now") {
                    onDismiss()
                    isPresented = false
                }

                Button("Enable") {
                    _ = LaunchAtLogin.enable()
                    onDismiss()
                    isPresented = false
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(28)
        .frame(width: 420)
    }
}
