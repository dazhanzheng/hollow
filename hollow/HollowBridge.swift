import Foundation

/// Swift-side wrapper that manages the HollowCore lifecycle.
/// Constructs the database path in Application Support and holds
/// a reference to the Rust-backed HollowCore instance.
class HollowBridge {
    static let shared = HollowBridge()

    private var core: HollowCore?

    var isReady: Bool { core != nil }

    private init() {
        do {
            let dbPath = try Self.databasePath()
            core = try HollowCore(dbPath: dbPath)
        } catch {
            print("HollowBridge init failed: \(error)")
            core = nil
        }
    }

    private static func databasePath() throws -> String {
        let appSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first!

        let hollowDir = appSupport.appendingPathComponent(
            "com.syncpulse.hollow",
            isDirectory: true
        )

        try FileManager.default.createDirectory(
            at: hollowDir,
            withIntermediateDirectories: true
        )

        return hollowDir.appendingPathComponent("hollow.db").path
    }

    func listFiles(limit: UInt32 = 20, offset: UInt32 = 0) -> [FileRecord] {
        guard let core else { return [] }
        do {
            return try core.listFiles(limit: limit, offset: offset)
        } catch {
            print("listFiles failed: \(error)")
            return []
        }
    }
}
