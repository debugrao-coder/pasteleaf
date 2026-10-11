import Foundation

@_silgen_name("pl_api_version") private func pl_api_version() -> UInt32
@_silgen_name("pl_open") private func pl_open(_ root: UnsafePointer<CChar>) -> UnsafeMutableRawPointer?
@_silgen_name("pl_request") private func pl_request(_ handle: UnsafeMutableRawPointer, _ json: UnsafePointer<CChar>) -> UnsafeMutablePointer<CChar>?
@_silgen_name("pl_free") private func pl_free(_ result: UnsafeMutablePointer<CChar>)
@_silgen_name("pl_close") private func pl_close(_ handle: UnsafeMutableRawPointer)

final class RustEngine: ClipboardEngine {
    let isDemo = false
    private let queue = DispatchQueue(label: "io.pasteleaf.core", qos: .userInitiated)
    private var handle: UnsafeMutableRawPointer?
    private let root: URL
    init(root: URL) { self.root = root }
    func request(_ command: [String: Any], completion: @escaping (Result<Any, Error>) -> Void) {
        queue.async { [self] in
            let result: Result<Any, Error>
            do {
                guard pl_api_version() == 2 else { throw CoreError(code: "abi_version", message: "共享核心版本不匹配，已停止访问数据。请安装配套的 PasteLeaf 版本。") }
                if handle == nil { handle = root.path.withCString { pl_open($0) } }
                guard let handle else { throw CoreError(code: "open_failed", message: "无法打开本地资料库。请检查目录权限和磁盘空间；原数据没有被替换。") }
                let data = try JSONSerialization.data(withJSONObject: command)
                guard let json = String(data: data, encoding: .utf8), let output = json.withCString({ pl_request(handle, $0) }) else {
                    throw CoreError(code: "request_failed", message: "共享核心没有返回结果，请重试。")
                }
                defer { pl_free(output) }
                let raw = Data(String(cString: output).utf8)
                guard let envelope = try JSONSerialization.jsonObject(with: raw) as? [String: Any] else { throw CoreError(code: "invalid_response", message: "共享核心返回了无法识别的数据。") }
                if envelope["ok"] as? Bool != true {
                    let error = envelope["error"] as? [String: Any] ?? [:]
                    throw CoreError(code: error["code"] as? String ?? "core", message: error["message"] as? String ?? "操作未完成。")
                }
                result = .success(envelope["data"] ?? NSNull())
            } catch { result = .failure(error) }
            DispatchQueue.main.async { completion(result) }
        }
    }
    deinit { if let handle { pl_close(handle) } }
}

/// Synthetic mode is explicit, never a fallback for an unavailable real core.
final class DemoEngine: ClipboardEngine {
    let isDemo = true
    private var items: [ClipItem]
    private var settings: [String: Any] = ["paused": true, "onboarding_complete": true, "appearance": "system", "hotkey": "option+command+v"]
    private var categories = [ClipCategory(id: "ideas", name: "灵感", color: DesignTokens.categoryColors[2]), ClipCategory(id: "work", name: "工作", color: DesignTokens.categoryColors[0]), ClipCategory(id: "snippets", name: "常用片段", color: DesignTokens.categoryColors[3])]
    init(now: Int64 = Int64(Date().timeIntervalSince1970 * 1000)) {
        let imagePath = Bundle.main.url(forResource: "demo-landscape", withExtension: "png")?.path
        let noteURL = Bundle.main.url(forResource: "Synthetic Notes", withExtension: "txt")
        let outlineURL = Bundle.main.url(forResource: "Synthetic Outline", withExtension: "md")
        let demoFiles = [noteURL, outlineURL].compactMap { $0 }.map { ClipFile(name: $0.lastPathComponent, path: $0.path, size: UInt64((try? Data(contentsOf: $0).count) ?? 0), hash: $0.lastPathComponent) }
        items = [
            ClipItem(id: "demo-note", text: "让好想法，随手可得。\n\n一个轻盈的剪贴板，把灵感、链接和日常片段，好好收在一起。", tags: ["产品灵感"], source: "备忘录", copiedAt: now - 60000),
            ClipItem(id: "demo-image", kind: "image", text: "山色 · 合成预览", imagePath: imagePath, category: "ideas", tags: ["视觉灵感"], source: "合成图库", copiedAt: now - 120000, available: imagePath != nil),
            ClipItem(id: "demo-link", text: "https://github.com/debugrao-coder/pasteleaf", category: "work", tags: ["开源"], source: "Safari", copiedAt: now - 180000),
            ClipItem(id: "demo-code", text: "const idea = {\n  title: 'Make room for good ideas',\n  saved: true,\n  synced: 'when you are ready'\n};", category: "snippets", tags: ["代码"], source: "Visual Studio Code", copiedAt: now - 420000),
            ClipItem(id: "demo-rich", kind: "rich_text", text: "设计不是装饰\n\n把复杂留在背后，把顺手放在眼前。\n每一个细节，都值得认真。", html: "<h2>设计不是装饰</h2><p>把复杂留在背后，把顺手放在眼前。</p>", category: "ideas", tags: ["设计"], source: "Pages", copiedAt: now - 600000),
            ClipItem(id: "demo-files", kind: "files", text: "品牌资源 · 设计交付", files: [ClipFile(name: "PasteLeaf — 视觉设计规范.pdf", path: "", size: 2_480_000, hash: "demo-file")], source: "访达", copiedAt: now - 960000, available: false),
            ClipItem(id: "demo-available-files", kind: "files", text: "合成文件组", files: demoFiles, category: "work", source: "合成文件", copiedAt: now - 1200000, available: !demoFiles.isEmpty),
            ClipItem(id: "demo-long", text: "所有值得保留的片段，都有自己的位置。\n\n支持中文搜索、收藏分类和标签；即使新的内容到来，你正在查看的卡片也不会被打断。", category: "ideas", source: "备忘录", copiedAt: now - 1800000)
        ]
    }
    func request(_ command: [String: Any], completion: @escaping (Result<Any, Error>) -> Void) {
        do {
            let op = command["op"] as? String ?? ""
            var output: Any = [:]
            switch op {
            case "list":
                let query = (command["query"] as? String ?? "").lowercased()
                let category = command["category"] as? String
                let filtered = items.filter { item in (query.isEmpty || (item.text + " " + item.tags.joined(separator: " ") + " " + item.source).lowercased().contains(query)) && (category == nil || (category == "favorites" ? item.category != nil : item.category == category)) }
                let offset = min(max(0, command["offset"] as? Int ?? 0), filtered.count)
                let end = min(filtered.count, offset + (command["limit"] as? Int ?? 50))
                output = ["items": try asJSON(Array(filtered[offset..<end])), "total": filtered.count]
            case "status": output = ["sync_state": "disconnected", "detail": "演示模式 · 未读取系统剪贴板，未连接 iCloud", "pending": 0, "conflicts": 0, "peer_receipts": 0, "paused": true, "settings": settings, "categories": try asJSON(categories)]
            case "get":
                guard let item = items.first(where: { $0.id == command["id"] as? String }) else { throw CoreError(code: "not_found", message: "记录不存在。") }
                output = try asJSON(item)
            case "settings":
                if let values = command["values"] as? [String: Any] { settings.merge(values) { _, new in new } }
                output = settings
            case "favorite":
                if let i = items.firstIndex(where: { $0.id == command["id"] as? String }) { items[i].category = command["category"] as? String }
            case "tags":
                if let i = items.firstIndex(where: { $0.id == command["id"] as? String }) { items[i].tags = command["tags"] as? [String] ?? [] }
            case "delete": items.removeAll { $0.id == command["id"] as? String }
            case "clear_local": items.removeAll { command["all"] as? Bool == true || $0.category == nil }
            case "category_create": categories.append(ClipCategory(id: UUID().uuidString, name: command["name"] as? String ?? "收藏夹", color: command["color"] as? String ?? DesignTokens.categoryColors[0]))
            case "category_rename": if let i = categories.firstIndex(where: { $0.id == command["id"] as? String }) { categories[i].name = command["name"] as? String ?? categories[i].name }
            case "category_delete": categories.removeAll { $0.id == command["id"] as? String }
            case "conflicts": output = []
            default: throw CoreError(code: "demo_only", message: "演示模式不会执行剪贴板、iCloud 或系统设置操作。请退出后正常启动。")
            }
            let resolved = output
            DispatchQueue.main.async { completion(.success(resolved)) }
        } catch { DispatchQueue.main.async { completion(.failure(error)) } }
    }
    private func asJSON<T: Encodable>(_ object: T) throws -> Any { try JSONSerialization.jsonObject(with: JSONEncoder().encode(object)) }
}
