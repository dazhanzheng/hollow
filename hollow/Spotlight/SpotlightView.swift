// hollow/Spotlight/SpotlightView.swift
import SwiftUI

/// SwiftUI content of the Spotlight panel. Hosted by `NSHostingView` inside
/// `SpotlightPanel`. Reads state directly from an observed coordinator and
/// drives all actions through it.
struct SpotlightView: View {
    @Bindable var coordinator: SpotlightCoordinator
    @FocusState private var fieldFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            // Hidden button so `.keyboardShortcut(.return, modifiers: .command)`
            // gets picked up by the focus system. `.onKeyPress` doesn't expose
            // modifier chords reliably on macOS, and a hidden button is the
            // stable pattern used elsewhere in SwiftUI for cmd-return style.
            Button("Reveal") { coordinator.revealSelected() }
                .keyboardShortcut(.return, modifiers: .command)
                .hidden()
                .frame(width: 0, height: 0)

            searchField
            if !coordinator.query.isEmpty || !coordinator.results.isEmpty {
                resultsSection
            } else {
                emptyState
            }
        }
        .frame(width: 680)
        .background(.ultraThinMaterial)
        .clipShape(RoundedRectangle(cornerRadius: 22))
        .onAppear { fieldFocused = true }
        .onChange(of: coordinator.isVisible) { _, visible in
            if visible { fieldFocused = true }
        }
        // ↵ — open the selected file
        .onKeyPress(.return) {
            if coordinator.selectedResult != nil {
                coordinator.openSelected()
                return .handled
            }
            return .ignored
        }
        // ↓ — move selection down
        .onKeyPress(.downArrow) {
            coordinator.moveSelectionDown()
            return .handled
        }
        // ↑ — move selection up
        .onKeyPress(.upArrow) {
            coordinator.moveSelectionUp()
            return .handled
        }
        // ESC — hide
        .onExitCommand {
            coordinator.hide()
        }
    }

    private var searchField: some View {
        HStack(spacing: 12) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 20, weight: .regular))
                .foregroundStyle(.tertiary)

            TextField("Search hollow...", text: Binding(
                get: { coordinator.query },
                set: { coordinator.onQueryChange($0) }
            ))
            .textFieldStyle(.plain)
            .font(.system(size: 24, weight: .regular))
            .focused($fieldFocused)
        }
        .padding(.horizontal, 24)
        .frame(height: 64)
    }

    private var resultsSection: some View {
        VStack(spacing: 4) {
            if coordinator.results.isEmpty {
                Text("No matches")
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
                    .frame(height: 56)
            } else {
                ForEach(Array(coordinator.results.enumerated()), id: \.offset) { index, result in
                    SpotlightResultRow(
                        result: result,
                        isSelected: index == coordinator.selectedIndex
                    )
                    .onTapGesture {
                        coordinator.selectedIndex = index
                        coordinator.openSelected()
                    }
                    .onHover { hovering in
                        if hovering { coordinator.selectedIndex = index }
                    }
                }
            }
        }
        .padding(.vertical, 8)
    }

    private var emptyState: some View {
        Text("Start typing to search...")
            .font(.system(size: 13))
            .foregroundStyle(.tertiary)
            .frame(maxWidth: .infinity)
            .frame(height: 56)
            .padding(.vertical, 8)
    }
}
