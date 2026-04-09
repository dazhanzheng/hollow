import Foundation
import os

final class RustLogRelay {
    static let shared = RustLogRelay()

    private var timer: Timer?
    private var lastSeenId: UInt64 = 0
    private let logger = HollowLogger.rustCore

    private init() {}

    func start() {
        pullLogs()
        DispatchQueue.main.async { [weak self] in
            self?.timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
                self?.pullLogs()
            }
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    private func pullLogs() {
        let entries = HollowBridge.shared.getLogs(sinceId: lastSeenId)
        for entry in entries {
            lastSeenId = entry.id
            switch entry.level {
            case .debug:
                logger.debug("[\(entry.target)] \(entry.message)")
            case .info:
                logger.info("[\(entry.target)] \(entry.message)")
            case .warn:
                logger.warning("[\(entry.target)] \(entry.message)")
            case .error:
                logger.error("[\(entry.target)] \(entry.message)")
            }
        }
    }
}
