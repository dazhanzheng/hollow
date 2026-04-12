import SwiftUI
import AppKit

/// Bring an app window to the front, covering both Dock-minimized windows
/// and Stage Manager "stashed" windows. SwiftUI's `openWindow` / `openSettings`
/// alone do neither: if the target window is in the Dock as a miniaturized
/// icon, or off to the side in the Stage Manager strip, a plain
/// `openWindow(id:)` appears to do nothing.
///
/// Strategy (covers both cases):
///  1. `NSApp.activate(ignoringOtherApps: true)` — required so Stage Manager
///     swaps this app into the active stage.
///  2. Call the passed-in opener — for single-instance `Window` / `Settings`
///     scenes this reuses the existing NSWindow; otherwise creates one.
///  3. Walk `NSApp.windows`, and for every window matching the predicate:
///     - `deminiaturize(nil)` if it was Dock-minimized (no-op otherwise),
///     - `makeKeyAndOrderFront(nil)` to pull it into the active stage.
///
/// Running the predicate *after* the opener means newly-created windows get
/// surfaced on first use too.
@MainActor
func surfaceWindow(
    open: () -> Void,
    matches: @escaping (NSWindow) -> Bool
) {
    NSApp.activate(ignoringOtherApps: true)
    open()
    for window in NSApp.windows where matches(window) {
        if window.isMiniaturized {
            window.deminiaturize(nil)
        }
        window.makeKeyAndOrderFront(nil)
    }
}

/// Heuristic: does this NSWindow belong to the SwiftUI `Settings` scene?
/// SwiftUI sets the window identifier to something like
/// `"com_apple_SwiftUI_Settings_window"` and the title to a localized
/// "App Settings" / "Settings" — match either for robustness across macOS
/// versions and locales.
func isSettingsWindow(_ window: NSWindow) -> Bool {
    if let id = window.identifier?.rawValue, id.contains("Settings") {
        return true
    }
    // Fallback: title match (localized). "Settings" on macOS 13+, sometimes
    // "<AppName> Settings" or "Preferences" on older systems.
    let title = window.title
    return title.localizedCaseInsensitiveContains("settings")
        || title.localizedCaseInsensitiveContains("preferences")
}

/// Match a Window scene by its scene id (the `id:` passed to `Window(_:id:)`).
/// SwiftUI encodes the scene id into the NSWindow identifier.
func isWindowWithSceneId(_ sceneId: String) -> (NSWindow) -> Bool {
    { window in
        window.identifier?.rawValue.contains(sceneId) == true
    }
}

/// Compact status panel shown when the user clicks the menu-bar icon.
/// Lives in the `MenuBarExtra(... style: .window)` scene in `hollowApp`.
struct MenuBarView: View {
    @Environment(IngestionService.self) private var ingestion
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            Divider()
            stats
            if !ingestion.recentFiles.isEmpty {
                Divider()
                recent
            }
            Divider()
            actions
        }
        .frame(width: 280)
        .padding(.vertical, 8)
    }

    // MARK: - Sections

    private var header: some View {
        HStack(spacing: 8) {
            Image(systemName: "archivebox.fill")
                .foregroundStyle(.tint)
                .font(.title3)
            VStack(alignment: .leading, spacing: 1) {
                Text("Hollow")
                    .font(.headline)
                HStack(spacing: 5) {
                    Circle()
                        .fill(ingestion.isWatching ? .green : .gray)
                        .frame(width: 6, height: 6)
                    Text(ingestion.isWatching
                         ? String(localized: "Watching Inbox")
                         : String(localized: "Paused"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 6)
    }

    private var stats: some View {
        VStack(alignment: .leading, spacing: 6) {
            statRow(
                label: String(localized: "Files tracked"),
                value: "\(ingestion.totalIngested)",
                color: .primary
            )
            if ingestion.extractionsInFlight > 0 {
                statRow(
                    label: String(localized: "Extracting"),
                    value: "\(ingestion.extractionsInFlight)",
                    color: .orange
                )
            }
            statRow(
                label: String(localized: "Extracted"),
                value: "\(ingestion.extractionsCompleted)",
                color: .green
            )
            if ingestion.extractionsFailed > 0 {
                statRow(
                    label: String(localized: "Failed"),
                    value: "\(ingestion.extractionsFailed)",
                    color: .red
                )
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
    }

    private func statRow(label: String, value: String, color: Color) -> some View {
        HStack {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
            Spacer()
            Text(value)
                .font(.caption.monospacedDigit().weight(.medium))
                .foregroundStyle(color)
        }
    }

    private var recent: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Recent")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            ForEach(Array(ingestion.recentFiles.prefix(3).enumerated()), id: \.offset) { _, name in
                HStack(spacing: 5) {
                    Image(systemName: "doc")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                    Text(name)
                        .font(.caption)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
    }

    private var actions: some View {
        VStack(spacing: 0) {
            menuButton(
                title: String(localized: "Open Main Window"),
                systemImage: "macwindow"
            ) {
                surfaceWindow(
                    open: { openWindow(id: "main") },
                    matches: isWindowWithSceneId("main")
                )
            }

            menuButton(
                title: String(localized: "Open Inbox in Finder"),
                systemImage: "folder"
            ) {
                NSWorkspace.shared.selectFile(
                    nil,
                    inFileViewerRootedAtPath: FileWatcher.inboxURL.path
                )
            }

            menuButton(
                title: ingestion.isWatching
                    ? String(localized: "Pause Watching")
                    : String(localized: "Resume Watching"),
                systemImage: ingestion.isWatching ? "pause.circle" : "play.circle"
            ) {
                if ingestion.isWatching {
                    ingestion.stop()
                } else {
                    ingestion.start()
                }
            }

            menuButton(
                title: String(localized: "Settings…"),
                systemImage: "gearshape"
            ) {
                surfaceWindow(
                    open: { openSettings() },
                    matches: isSettingsWindow
                )
            }

            Divider()
                .padding(.vertical, 2)

            menuButton(
                title: String(localized: "Quit Hollow"),
                systemImage: "power"
            ) {
                NSApplication.shared.terminate(nil)
            }
        }
        .padding(.horizontal, 6)
    }

    private func menuButton(
        title: String,
        systemImage: String,
        action: @escaping () -> Void
    ) -> some View {
        MenuBarButton(title: title, systemImage: systemImage, action: action)
    }
}

/// A menu-bar-style button with a native-feeling hover highlight. SwiftUI's
/// default `.plain` button style has no hover feedback, and `.menu` styling
/// isn't available in a `MenuBarExtra(style: .window)` panel.
private struct MenuBarButton: View {
    let title: String
    let systemImage: String
    let action: () -> Void

    @State private var isHovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Image(systemName: systemImage)
                    .frame(width: 16)
                    .foregroundStyle(isHovered ? Color.white : .secondary)
                Text(title)
                    .font(.callout)
                    .foregroundStyle(isHovered ? Color.white : .primary)
                Spacer()
            }
            .contentShape(Rectangle())
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .background(
                RoundedRectangle(cornerRadius: 5)
                    .fill(isHovered ? Color.accentColor : .clear)
            )
        }
        .buttonStyle(.plain)
        .onHover { hovering in
            isHovered = hovering
        }
    }
}
