import Foundation

/// Safe content metadata from the core; unlike a local file, this has no source path.
struct ClipAttachment: Codable, Identifiable, Hashable {
    var name: String
    var size: UInt64
    var hash: String
    var id: String { hash + name }
    var checksumDescription: String { "BLAKE3：" + hash }
}

struct ClipFile: Codable, Identifiable, Hashable {
    var name: String
    var path: String
    var size: UInt64
    var hash: String
    var id: String { hash + name }
    var checksumDescription: String { "BLAKE3：" + hash }
}

struct ClipItem: Codable, Identifiable, Hashable {
    var id: String
    var kind: String
    var text: String
    var html: String?
    var rtf: String?
    var files: [ClipFile]
    var image: ClipAttachment?
    var imagePath: String?
    var category: String?
    var tags: [String]
    var source: String
    var copiedAt: Int64
    var available: Bool
    enum CodingKeys: String, CodingKey {
        case id, kind, text, html, rtf, files, image, category, tags, source, available
        case imagePath = "image_path", copiedAt = "copied_at"
    }
    init(id: String, kind: String = "text", text: String, html: String? = nil, rtf: String? = nil, files: [ClipFile] = [], image: ClipAttachment? = nil, imagePath: String? = nil, category: String? = nil, tags: [String] = [], source: String = "PasteLeaf", copiedAt: Int64 = 0, available: Bool = true) {
        self.id = id; self.kind = kind; self.text = text; self.html = html; self.rtf = rtf
        self.files = files; self.image = image; self.imagePath = imagePath; self.category = category; self.tags = tags
        self.source = source; self.copiedAt = copiedAt; self.available = available
    }
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? "text"
        text = try c.decodeIfPresent(String.self, forKey: .text) ?? ""
        html = try c.decodeIfPresent(String.self, forKey: .html)
        rtf = try c.decodeIfPresent(String.self, forKey: .rtf)
        files = try c.decodeIfPresent([ClipFile].self, forKey: .files) ?? []
        image = try c.decodeIfPresent(ClipAttachment.self, forKey: .image)
        imagePath = try c.decodeIfPresent(String.self, forKey: .imagePath)
        category = try c.decodeIfPresent(String.self, forKey: .category)
        tags = try c.decodeIfPresent([String].self, forKey: .tags) ?? []
        source = try c.decodeIfPresent(String.self, forKey: .source) ?? "未知来源"
        copiedAt = try c.decodeIfPresent(Int64.self, forKey: .copiedAt) ?? 0
        available = try c.decodeIfPresent(Bool.self, forKey: .available) ?? true
    }
    var isLink: Bool {
        guard kind == "text", let url = URL(string: text.trimmingCharacters(in: .whitespacesAndNewlines)) else { return false }
        return ["https", "http"].contains(url.scheme?.lowercased() ?? "") && url.host != nil && !text.contains("\n")
    }
    var isCode: Bool { kind == "text" && (text.contains("func ") || text.contains("const ") || text.contains("fn ") || text.contains("import ") || text.contains("def ") || text.contains("{\n")) }
    var typeTitle: String {
        switch kind { case "image": return "图片"; case "rich_text": return "富文本"; case "files": return "文件"; default: return isLink ? "链接" : (isCode ? "代码" : "文本") }
    }
    var symbol: String {
        switch kind { case "image": return "photo"; case "rich_text": return "textformat"; case "files": return "doc.on.doc"; default: return isLink ? "link" : (isCode ? "chevron.left.forwardslash.chevron.right" : "text.alignleft") }
    }
    var title: String {
        if kind == "files" { return files.first?.name ?? "等待文件" }
        if kind == "image" { return text.isEmpty ? "一张图片" : text }
        return text.split(separator: "\n", omittingEmptySubsequences: true).first.map(String.init) ?? "无纯文本内容"
    }
    var footer: String {
        if !available { return "等待附件下载" }
        if kind == "image" {
            return image.map { "图片 · " + ByteCountFormatter.string(fromByteCount: Int64(clamping: $0.size), countStyle: .file) } ?? "图片"
        }
        if kind == "files" { return "\(files.count) 个文件 · \(ByteCountFormatter.string(fromByteCount: Int64(clamping: files.reduce(0) { $0 + $1.size }), countStyle: .file))" }
        return "\(text.count.formatted()) 字符"
    }
}

struct ClipCategory: Codable, Identifiable, Hashable { var id: String; var name: String; var color: String }
struct ClipPage: Decodable { var items: [ClipItem]; var total: Int }
struct CoreError: LocalizedError {
    let code: String; let message: String
    var errorDescription: String? {
        switch code {
        case "sync_legacy_library": return "所选目录是旧版 v1 同步库，不能直接加入 v2。请先升级两端；由第一台设备明确创建独立 v2 库，另一端再加入。旧文件保持不变，旧云端历史不会自动迁入。"
        case "sync_library_missing": return "没有找到 v2 同步库。第一台设备请选择“创建新同步库”；若另一端已创建，请等 iCloud 下载完成后重新检查目录。"
        case "sync_path_missing": return "所选目录不存在或尚未下载到本机，请在 Finder 中确认目录后重新选择。"
        case "sync_path_invalid": return "请选择真实的本机目录，不支持文件、符号链接或不可用的路径。"
        case "sync_access_denied": return "无法读取同步目录，请检查所选目录的访问权限。原历史没有被替换。"
        case "pending_hydration": return "文件尚未下载到本机。请在 Finder 中下载并保留在本机后重试。"
        case "sync_unavailable": return "同步目录暂时不可用，请检查 iCloud 下载状态、磁盘和目录权限后重试。"
        case "sync_invalid_library": return "同步库描述文件无效或损坏。请核对所选目录和客户端版本，不要覆盖原库。"
        case "protocol_version": return "同步协议版本不受支持，请安装与该同步库匹配的客户端版本。原库未被覆盖。"
        case "authentication": return "解密验证失败：密码不匹配，或同步文件未通过完整性校验。请核对同步密码；不要输入 Apple ID 密码。"
        case "sync_exists": return "目标 v2 目录已有文件，不能覆盖创建。请选择“加入已有同步库”，或选择新的专用目录。"
        case "already_connected": return "本机已连接一个同步库。更换前请明确断开当前连接；不要覆盖现有配置。"
        default: return message
        }
    }
}

/// All persistence and synchronization rules live in Rust. This boundary remains injectable.
protocol ClipboardEngine: AnyObject {
    var isDemo: Bool { get }
    func request(_ command: [String: Any], completion: @escaping (Result<Any, Error>) -> Void)
}

func decodeCore<T: Decodable>(_ value: Any, as type: T.Type) throws -> T {
    try JSONDecoder().decode(type, from: JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed]))
}

struct HotkeySpec: Equatable {
    static let defaultValue = "option+command+v"
    static let keyCodes: [String: UInt32] = ["a":0,"s":1,"d":2,"f":3,"h":4,"g":5,"z":6,"x":7,"c":8,"v":9,"b":11,"q":12,"w":13,"e":14,"r":15,"y":16,"t":17,"1":18,"2":19,"3":20,"4":21,"6":22,"5":23,"9":25,"7":26,"8":28,"0":29,"o":31,"u":32,"i":34,"p":35,"l":37,"j":38,"k":40,"n":45,"m":46]
    let key: String
    let command: Bool
    let option: Bool
    let control: Bool
    let shift: Bool
    static func parse(_ input: String) throws -> HotkeySpec {
        let aliases = ["cmd": "command", "alt": "option", "ctrl": "control"]
        let parts = input.lowercased().split(separator: "+", omittingEmptySubsequences: false).map {
            let part = $0.trimmingCharacters(in: .whitespacesAndNewlines)
            return aliases[part] ?? part
        }
        let known = Set(["command", "option", "control", "shift"])
        let keys = parts.filter { !known.contains($0) }
        guard parts.count >= 2, Set(parts).count == parts.count,
              keys.count == 1, keyCodes[keys[0]] != nil,
              parts.contains(where: { ["command", "option", "control"].contains($0) }) else {
            throw CoreError(code: "hotkey", message: "请使用一个英文字母或数字，并至少包含 ⌃ Control、⌥ Option 或 ⌘ Command；不能只用普通按键或 Shift。")
        }
        return HotkeySpec(key: keys[0], command: parts.contains("command"), option: parts.contains("option"), control: parts.contains("control"), shift: parts.contains("shift"))
    }
    var canonical: String { ([control ? "control" : nil, option ? "option" : nil, shift ? "shift" : nil, command ? "command" : nil].compactMap { $0 } + [key]).joined(separator: "+") }
    var display: String { (control ? "⌃" : "") + (option ? "⌥" : "") + (shift ? "⇧" : "") + (command ? "⌘" : "") + key.uppercased() }
}

/// Pure logic: user-triggered sequencing never sends Return and never auto-advances.
enum PasteBatch {
    case text(String)
    case files([ClipFile])
    case single(ClipItem)
    case sequence([ClipItem])
    static func make(_ items: [ClipItem], plain: Bool = false) throws -> PasteBatch {
        guard !items.isEmpty else { throw CoreError(code: "empty_selection", message: "先选择要粘贴的内容。") }
        guard items.allSatisfy(\.available) else { throw CoreError(code: "attachments_pending", message: "附件尚未完整下载，请稍后重试。") }
        if plain {
            guard items.contains(where: { !$0.text.isEmpty }) else { throw CoreError(code: "no_plain_text", message: "所选内容没有可用的纯文本回退，未改写剪贴板。") }
            return .text(items.map(\.text).joined(separator: "\n"))
        }
        if items.count == 1 { return .single(items[0]) }
        if items.allSatisfy({ ["text", "rich_text"].contains($0.kind) }) { return .text(items.map(\.text).joined(separator: "\n")) }
        if items.allSatisfy({ $0.kind == "files" }) { return .files(items.flatMap(\.files)) }
        return .sequence(items)
    }
}

/// Translate known status explanations, never discard an actual core failure or unknown detail.
enum SyncStatusCopy {
    static func detail(state: String, raw: String, hasError: Bool, isDemo: Bool, isTestLibrary: Bool) -> String {
        let standard: [String: String] = [
            "disconnected": "仅保留本机历史，尚未连接 iCloud Drive。",
            "paused": "同步已暂停，本机历史仍保留。",
            "locked": "输入同步密码，解锁加密同步库。",
            "waiting_attachments": "等待 iCloud Drive 下载附件或前序同步记录。",
            "pending": "本机变更已排队，等待写入同步目录。",
            "published": "已写入本机同步目录；尚未确认云端送达。",
            "peer_imported": "已收到另一台设备导入所有已发布操作的回执。",
            "conflict": "存在并发版本，请明确选择；没有静默覆盖。",
            "error": "同步需要处理，本机历史仍保留。"
        ]
        let knownRaw = Set([
            "Local history only; iCloud Drive is not connected",
            "Sync is paused; local history is retained",
            "Unlock the encrypted sync library",
            "Waiting for iCloud to download a sync file",
            "Waiting for attachments or causal history to arrive through iCloud Drive",
            "Local changes are queued for publication",
            "Written to the local sync folder; cloud delivery is not yet confirmed",
            "Another device acknowledged importing all published operations",
            "Concurrent versions require a choice; none were silently overwritten",
            "Sync needs attention; local history is retained"
        ])
        let localized = isTestLibrary ? "隔离验证：不会连接 iCloud 或读取系统剪贴板。" : (isDemo ? "安全演示：仅使用合成数据，不连接 iCloud 或读取系统剪贴板。" : (standard[state] ?? "未知同步状态：\(state)"))
        if hasError || state == "error" { return raw.isEmpty ? localized : localized + "\n具体原因：" + raw }
        if !isDemo && !isTestLibrary && !raw.isEmpty && !knownRaw.contains(raw) { return localized + "\n补充信息：" + raw }
        return localized
    }
}
