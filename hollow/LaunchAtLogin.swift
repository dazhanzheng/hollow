import Foundation
import ServiceManagement
import os

/// Thin wrapper around `SMAppService.mainApp` so the rest of the app can
/// check/toggle launch-at-login without littering imports and error handling
/// across views.
///
/// macOS 13+ API. Since hollow targets macOS 26.2+, the older
/// `SMLoginItemSetEnabled` / `LSSharedFileList` code paths don't exist here.
enum LaunchAtLogin {

    /// Whether the current app is registered as a login item.
    static var isEnabled: Bool {
        SMAppService.mainApp.status == .enabled
    }

    /// Raw status, useful for UI states like "requires approval in System Settings".
    static var status: SMAppService.Status {
        SMAppService.mainApp.status
    }

    /// Register the app as a login item. Returns true on success.
    /// The first call may open a macOS approval dialog / redirect the user
    /// to System Settings → General → Login Items if the system requires it.
    @discardableResult
    static func enable() -> Bool {
        let service = SMAppService.mainApp
        do {
            try service.register()
            HollowLogger.app.info("Launch-at-login registered")
            return true
        } catch {
            HollowLogger.app.error(
                "Launch-at-login register failed: \(error.localizedDescription, privacy: .public)"
            )
            return false
        }
    }

    /// Unregister the app as a login item.
    @discardableResult
    static func disable() -> Bool {
        let service = SMAppService.mainApp
        do {
            try service.unregister()
            HollowLogger.app.info("Launch-at-login unregistered")
            return true
        } catch {
            HollowLogger.app.error(
                "Launch-at-login unregister failed: \(error.localizedDescription, privacy: .public)"
            )
            return false
        }
    }

    /// Set to a desired enabled state — convenience for a SwiftUI Toggle.
    static func setEnabled(_ enabled: Bool) {
        if enabled {
            _ = enable()
        } else {
            _ = disable()
        }
    }

    /// If the user was sent to System Settings to approve the login item,
    /// this jumps there directly.
    static func openSystemSettingsLoginItems() {
        SMAppService.openSystemSettingsLoginItems()
    }
}
