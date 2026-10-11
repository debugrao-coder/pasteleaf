import Combine
import Foundation

/// Preflight only: never sends a password, connects a library, or creates a directory.
@MainActor
final class SyncConnectionDraft: ObservableObject {
    @Published private(set) var directory = ""
    @Published private(set) var state = "unselected"
    @Published private(set) var error: String?
    @Published private(set) var isChecking = false
    private let engine: ClipboardEngine
    private var generation = 0
    init(engine: ClipboardEngine) { self.engine = engine }

    var canJoin: Bool { !isChecking && state == "v2" }
    var canCreate: Bool { !isChecking && ["missing", "legacy_v1"].contains(state) }
    var guidance: String {
        switch state {
        case "v2": return "找到 v2 同步库。请选择“加入已有同步库”，使用创建该库时设置的同步密码。"
        case "legacy_v1": return "这是旧版 v1 同步库，不能直接加入 v2。双端升级后，由第一台设备明确选择“创建新同步库”；旧文件原样保留，另一端随后加入新建的 PasteLeaf-v2 目录。旧云端历史不会自动迁入。"
        case "missing": return "未找到 v2 同步库。第一台设备可选择“创建新同步库”；若另一台已创建，请先等 iCloud 下载完成，再重新检查，不要重复创建。"
        case "pending_download": return "同步库文件尚未下载到本机。请在 Finder 中将该目录下载保留在本机，再重新检查。"
        case "checking": return "正在检查目录与协议版本，不会创建或修改文件…"
        case "error": return error ?? "目录检查失败，请重新选择。"
        default: return "第一台设备选择“创建新同步库”；其他设备只能加入已创建且已下载到本机的 v2 同步库。"
        }
    }
    func selectDirectory(_ path: String) {
        generation += 1
        let request = generation
        directory = path; error = nil
        guard !path.isEmpty else { state = "unselected"; isChecking = false; return }
        state = "checking"; isChecking = true
        engine.request(["op": "sync_inspect", "directory": path]) { [weak self] result in
            guard let self, request == self.generation else { return }
            self.isChecking = false
            switch result {
            case .success(let value):
                guard let data = value as? [String: Any], let state = data["state"] as? String,
                      ["v2", "legacy_v1", "missing", "pending_download"].contains(state) else {
                    self.state = "error"; self.error = "无法识别目录检查结果，请安装配套版本。"; return
                }
                self.state = state
            case .failure(let error): self.state = "error"; self.error = error.localizedDescription
            }
        }
    }
}
