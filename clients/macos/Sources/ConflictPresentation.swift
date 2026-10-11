import CryptoKit
import Foundation

/// Read-only conflict presentation. Original payloads are not rendered or logged.
struct ConflictGroup: Decodable, Identifiable {
    let entity: String
    let versions: [ConflictVersion]
    var id: String { entity }
}

struct ConflictFormatDigest: Identifiable, Equatable {
    let format: String
    let byteCount: Int?
    let sha256: String?
    let invalid: Bool
    var id: String { format }
    var summary: String {
        if invalid { return "\(format) · 数据无效，无法比较原始字节" }
        guard let byteCount else { return "\(format) · 未提供" }
        return "\(format) · 已保留 · \(byteCount.formatted()) 字节"
    }
    init(format: String, value: String?, base64: Bool = false) {
        self.format = format
        guard let value else { byteCount = nil; sha256 = nil; invalid = false; return }
        guard let bytes = base64 ? Data(base64Encoded: value) : Data(value.utf8) else {
            byteCount = nil; sha256 = nil; invalid = true; return
        }
        byteCount = bytes.count
        sha256 = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
        invalid = false
    }
}

struct ConflictVersion: Decodable, Identifiable {
    let opID: String
    let device: String
    let deleted: Bool
    let record: ClipItem?
    let category: ClipCategory?
    let formats: [ConflictFormatDigest]
    let bodyCharacterCount: Int
    var id: String { opID }
    enum CodingKeys: String, CodingKey { case opID = "op_id", device, deleted, record, category }
    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        opID = try values.decode(String.self, forKey: .opID)
        device = try values.decodeIfPresent(String.self, forKey: .device) ?? "未知设备"
        deleted = try values.decodeIfPresent(Bool.self, forKey: .deleted) ?? false
        record = try values.decodeIfPresent(ClipItem.self, forKey: .record)
        category = try values.decodeIfPresent(ClipCategory.self, forKey: .category)
        bodyCharacterCount = record?.text.count ?? 0
        if let record, record.kind == "rich_text" || record.html != nil || record.rtf != nil {
            formats = [ConflictFormatDigest(format: "HTML", value: record.html), ConflictFormatDigest(format: "RTF", value: record.rtf, base64: true)]
        } else { formats = [] }
    }
    var typeTitle: String { record?.typeTitle ?? (category != nil ? "收藏夹" : (deleted ? "删除操作" : "内容更新")) }
    var statusTitle: String { deleted ? "删除版本" : "保留内容版本" }
    var tagsLabel: String { record.map { $0.tags.isEmpty ? "无标签" : $0.tags.map { "#" + $0 }.joined(separator: "  ") } ?? "不适用" }
    func categoryLabel(_ categories: [ClipCategory]) -> String {
        guard let record else { return category?.name ?? "不适用" }
        guard let id = record.category else { return "未收藏" }
        return categories.first(where: { $0.id == id })?.name ?? "未找到收藏夹（\(id)）"
    }
    var capturedAtLabel: String {
        guard let record, record.copiedAt > 0 else { return "未提供捕获时间" }
        return Date(timeIntervalSince1970: Double(record.copiedAt) / 1000).formatted(date: .numeric, time: .complete)
    }
}
