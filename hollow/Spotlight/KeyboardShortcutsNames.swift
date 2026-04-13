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

/// A curated set of hotkey presets we expose in Settings. Replaces the
/// free-form `KeyboardShortcuts.Recorder` because users found "press a key
/// combo to record" confusing — a picker with named options is friendlier
/// when there are only a handful of sensible defaults anyway.
enum SpotlightHotkeyChoice: String, CaseIterable, Identifiable {
    case optionSpace
    case controlSpace
    case commandShiftSpace
    case controlOptionSpace
    case hyperSpace          // ⌃⌥⌘ + Space
    case hyperH              // ⌃⌥⌘ + H
    case disabled

    var id: String { rawValue }

    /// Human-readable label shown in the Picker.
    var label: String {
        switch self {
        case .optionSpace:        return "⌥ Space"
        case .controlSpace:       return "⌃ Space"
        case .commandShiftSpace:  return "⌘ ⇧ Space"
        case .controlOptionSpace: return "⌃ ⌥ Space"
        case .hyperSpace:         return "⌃ ⌥ ⌘ Space"
        case .hyperH:             return "⌃ ⌥ ⌘ H"
        case .disabled:           return String(localized: "Disabled")
        }
    }

    /// Convert this choice to a `KeyboardShortcuts.Shortcut?`. `nil` means
    /// "no hotkey" — passing `nil` to `setShortcut(_:for:)` clears the binding.
    var shortcut: KeyboardShortcuts.Shortcut? {
        switch self {
        case .optionSpace:
            return .init(.space, modifiers: [.option])
        case .controlSpace:
            return .init(.space, modifiers: [.control])
        case .commandShiftSpace:
            return .init(.space, modifiers: [.command, .shift])
        case .controlOptionSpace:
            return .init(.space, modifiers: [.control, .option])
        case .hyperSpace:
            return .init(.space, modifiers: [.control, .option, .command])
        case .hyperH:
            return .init(.h, modifiers: [.control, .option, .command])
        case .disabled:
            return nil
        }
    }

    /// Best-effort match of the currently-stored shortcut against a preset.
    /// If the user has somehow configured a shortcut outside this list (e.g.
    /// from a previous build that used the Recorder), we fall back to the
    /// default `.optionSpace` so the Picker has something selected.
    static func current() -> SpotlightHotkeyChoice {
        let stored = KeyboardShortcuts.getShortcut(for: .spotlightSearch)
        for choice in Self.allCases where choice.shortcut == stored {
            return choice
        }
        return .optionSpace
    }
}
