// hollow/Spotlight/KeyboardShortcutsNames.swift
import AppKit
import KeyboardShortcuts

extension KeyboardShortcuts.Name {
    /// Global hotkey for the Spotlight-style search overlay. Default is
    /// ⌥Space (Option + Space) — chosen to rhyme with the system Spotlight
    /// shortcut (⌘Space) while avoiding the conflict. Users can rebind or
    /// clear this in Settings → General → Global Search.
    static let spotlightSearch = Self(
        "spotlightSearch",
        default: .init(.space, modifiers: [.option])
    )
}
