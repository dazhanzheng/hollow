// hollow/Spotlight/SpotlightPanel.swift
import AppKit
import SwiftUI

/// Borderless, non-activating HUD-style panel that hosts the Spotlight
/// search UI. Made key so the embedded `TextField` can receive focus, but
/// does NOT activate the app — clicking into the panel from another app
/// leaves that app's windows as they were.
final class SpotlightPanel: NSPanel {
    init(rootView: SpotlightView) {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 680, height: 60),
            styleMask: [.borderless, .nonactivatingPanel, .hudWindow],
            backing: .buffered,
            defer: false
        )
        self.isOpaque = false
        self.backgroundColor = .clear
        self.hasShadow = true
        self.level = .floating
        self.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        self.isMovable = false
        self.hidesOnDeactivate = false
        self.animationBehavior = .utilityWindow

        let host = NSHostingView(rootView: rootView)
        host.translatesAutoresizingMaskIntoConstraints = false
        let container = NSView()
        container.addSubview(host)
        NSLayoutConstraint.activate([
            host.topAnchor.constraint(equalTo: container.topAnchor),
            host.bottomAnchor.constraint(equalTo: container.bottomAnchor),
            host.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: container.trailingAnchor),
        ])
        self.contentView = container
    }

    /// Required so the embedded SwiftUI `TextField` can become first responder.
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }

    /// Position the panel centered horizontally on the current main screen,
    /// with its top 35% down from the top of the screen's visible frame.
    func positionCentered() {
        guard let screen = NSScreen.main ?? NSScreen.screens.first else { return }
        let visible = screen.visibleFrame
        let size = self.frame.size
        let x = visible.midX - size.width / 2
        let y = visible.maxY - size.height - visible.height * 0.35
        self.setFrameOrigin(NSPoint(x: x, y: y))
    }
}
