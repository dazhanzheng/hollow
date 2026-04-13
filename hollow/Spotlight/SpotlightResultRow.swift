// hollow/Spotlight/SpotlightResultRow.swift
import AppKit
import UniformTypeIdentifiers
import SwiftUI

/// One row in the Spotlight search results list. 52pt high, icon + filename
/// + snippet. Selection is driven by the parent view (no hover state of its
/// own — selection and hover are unified in the coordinator).
struct SpotlightResultRow: View {
    let result: SearchResult
    let isSelected: Bool

    var body: some View {
        HStack(spacing: 12) {
            fileIcon
                .frame(width: 36, height: 36)

            VStack(alignment: .leading, spacing: 2) {
                Text(result.fileName)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(isSelected ? Color.white : .primary)
                    .lineLimit(1)
                    .truncationMode(.middle)

                if !result.snippet.isEmpty {
                    Text(snippetPlain(result.snippet))
                        .font(.system(size: 12))
                        .foregroundStyle(isSelected ? Color.white.opacity(0.85) : .secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }

            Spacer(minLength: 0)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 8)
        .frame(height: 52)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 6)
                .fill(isSelected ? Color.accentColor : .clear)
                .padding(.horizontal, 8)
        )
        .contentShape(Rectangle())
    }

    /// Native file icon for the result's on-disk path. Falls back to a generic
    /// doc icon if the file no longer exists (e.g. moved after indexing).
    private var fileIcon: some View {
        let ws = NSWorkspace.shared
        let nsImage: NSImage
        if FileManager.default.fileExists(atPath: result.currentPath) {
            nsImage = ws.icon(forFile: result.currentPath)
        } else {
            nsImage = ws.icon(for: .data)
        }
        return Image(nsImage: nsImage)
            .resizable()
            .aspectRatio(contentMode: .fit)
    }

    /// FTS5 snippets are returned with `<b>...</b>` around hit terms. For v1
    /// we strip the tags and show plain text; rich highlighting can be added
    /// as an enhancement once the base layout is proven.
    private func snippetPlain(_ s: String) -> String {
        s.replacingOccurrences(of: "<b>", with: "")
         .replacingOccurrences(of: "</b>", with: "")
    }
}
