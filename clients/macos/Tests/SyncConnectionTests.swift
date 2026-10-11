import Foundation

final class InspectionEngine: ClipboardEngine {
    let isDemo = false
    var requests: [([String: Any], (Result<Any, Error>) -> Void)] = []
    func request(_ command: [String: Any], completion: @escaping (Result<Any, Error>) -> Void) { requests.append((command, completion)) }
}

@main
struct SyncConnectionTests {
    @MainActor static func main() async throws {
        var count = 0
        func check(_ value: @autoclosure () -> Bool, _ title: String) throws {
            guard value() else { throw CoreError(code: "test", message: title) }
            count += 1
        }
        let engine = InspectionEngine()
        let draft = SyncConnectionDraft(engine: engine)
        try check(!draft.canJoin && !draft.canCreate && engine.requests.isEmpty, "No access before directory selection")
        draft.selectDirectory("/synthetic/first")
        try check(draft.isChecking && !draft.canJoin && !draft.canCreate, "No connection while checking")
        try check(Set(engine.requests[0].0.keys) == ["op", "directory"] && engine.requests[0].0["op"] as? String == "sync_inspect", "Inspection never sends password or creates library")
        draft.selectDirectory("/synthetic/latest")
        engine.requests[1].1(.success(["state": "legacy_v1"]))
        engine.requests[0].1(.success(["state": "v2"]))
        try check(draft.state == "legacy_v1" && draft.directory == "/synthetic/latest", "Stale response cannot enable wrong library")
        try check(!draft.canJoin && draft.canCreate && draft.guidance.contains("不会自动迁入"), "Legacy blocks join and explains explicit creation boundary")
        draft.selectDirectory("/synthetic/v2")
        engine.requests[2].1(.success(["state": "v2"]))
        try check(draft.canJoin && !draft.canCreate, "Existing v2 prevents overwrite creation")
        draft.selectDirectory("/synthetic/pending")
        engine.requests[3].1(.success(["state": "pending_download"]))
        try check(!draft.canJoin && !draft.canCreate && draft.guidance.contains("下载"), "Pending cloud download cannot cause new library creation")
        draft.selectDirectory("/synthetic/empty")
        engine.requests[4].1(.success(["state": "missing"]))
        try check(!draft.canJoin && draft.canCreate && draft.guidance.contains("不要重复创建"), "First device versus delayed library is explicit")
        draft.selectDirectory("/synthetic/unreadable")
        engine.requests[5].1(.failure(CoreError(code: "sync_access_denied", message: "raw")))
        try check(draft.state == "error" && !draft.canCreate && draft.guidance.contains("访问权限"), "Read errors disable connect and translate safely")
        draft.selectDirectory("/synthetic/unknown")
        engine.requests[6].1(.success(["state": "unknown"]))
        try check(draft.state == "error" && !draft.canJoin, "Unknown protocol response fails closed")
        draft.selectDirectory("")
        try check(draft.state == "unselected" && !draft.isChecking && engine.requests.count == 7, "Cleared directory does not query disk")
        for code in ["sync_legacy_library", "sync_library_missing", "sync_path_missing", "sync_path_invalid", "sync_access_denied", "pending_hydration", "sync_unavailable", "sync_invalid_library", "protocol_version", "authentication", "sync_exists", "already_connected"] {
            let error = CoreError(code: code, message: "raw diagnostic")
            try check(error.localizedDescription != "raw diagnostic", "Chinese error mapping: " + code)
        }
        try check(CoreError(code: "other", message: "fallback").localizedDescription == "fallback", "Unknown error detail retained")

        let root = FileManager.default.temporaryDirectory.appendingPathComponent("pasteleaf-sync-preflight-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let legacy = root.appendingPathComponent("synthetic-legacy")
        try FileManager.default.createDirectory(at: legacy, withIntermediateDirectories: true)
        let marker = legacy.appendingPathComponent("pasteleaf-v1.json")
        let bytes = Data("{\"version\":1,\"synthetic\":true}".utf8)
        try bytes.write(to: marker)
        let real = RustEngine(root: root.appendingPathComponent("isolated-local"))
        func request(_ value: [String: Any]) async throws -> Any {
            try await withCheckedThrowingContinuation { continuation in
                real.request(value) { continuation.resume(with: $0) }
            }
        }
        let inspected = try await request(["op": "sync_inspect", "directory": legacy.path]) as? [String: Any]
        try check(inspected?["state"] as? String == "legacy_v1", "Real ABI recognizes legacy marker without password")
        let after = try Data(contentsOf: marker)
        try check(after == bytes && !FileManager.default.fileExists(atPath: legacy.appendingPathComponent("PasteLeaf-v2").path), "Inspection leaves old library unchanged")
        do {
            _ = try await request(["op": "sync_join", "directory": legacy.path, "password": "synthetic-test-only"])
            throw CoreError(code: "test", message: "Legacy join should fail")
        } catch let error as CoreError {
            try check(error.code == "sync_legacy_library" && error.localizedDescription.contains("旧版 v1"), "Actual legacy join gives actionable localized error")
        }
        let status = try await request(["op": "status"]) as? [String: Any]
        try check(status?["sync_state"] as? String == "disconnected", "Failed join never connects or publishes")
        print("PASS: \(count) sync preflight / localization / stale-response / real ABI assertions. Synthetic temporary data only.")
    }
}
