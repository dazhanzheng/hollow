import SwiftUI

struct SettingsView: View {
    private let inboxPath = FileWatcher.inboxURL.path
    private let dbPath: String = {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!
        return appSupport
            .appendingPathComponent("com.syncpulse.hollow/hollow.db")
            .path
    }()

    var body: some View {
        Form {
            Section("Storage") {
                LabeledContent("Inbox Folder") {
                    Text(inboxPath)
                        .textSelection(.enabled)
                        .foregroundStyle(.secondary)
                }
                LabeledContent("Database") {
                    Text(dbPath)
                        .textSelection(.enabled)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 450)
        .padding()
    }
}
