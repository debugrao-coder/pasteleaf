import AppKit
import SwiftUI

/// A compact native sheet title. Content surfaces do not apply decorative glass or cards.
struct SheetHeader: View {
    var title: String
    var subtitle: String? = nil
    var close: () -> Void
    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.headline).accessibilityAddTraits(.isHeader)
                if let subtitle {
                    Text(subtitle).font(.subheadline).foregroundStyle(.secondary)
                        .lineLimit(2).help(subtitle)
                }
            }
            Spacer(minLength: 8)
            Button(action: close) { Image(systemName: "xmark") }
                .buttonStyle(.borderless).controlSize(.small).help("关闭").accessibilityLabel("关闭")
        }
    }
}

/// Errors remain readable and selectable even when the core returns a long explanation.
struct NativeIssueMessage: View {
    let text: String
    var dismiss: (() -> Void)? = nil
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Label("操作未完成", systemImage: "exclamationmark.triangle")
                    .font(.callout.weight(.medium)).foregroundStyle(.red)
                Spacer()
                if let dismiss {
                    Button(action: dismiss) { Image(systemName: "xmark") }
                        .buttonStyle(.borderless).accessibilityLabel("关闭错误说明")
                }
            }
            ScrollView {
                Text(text).font(.callout).textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true).frame(maxWidth: .infinity, alignment: .leading)
            }.frame(maxHeight: 64)
        }.accessibilityElement(children: .contain)
    }
}

struct PreviewView: View {
    let item: ClipItem
    @ObservedObject var model: AppModel
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(spacing: 0) {
            SheetHeader(title: item.typeTitle + "预览", subtitle: "\(item.source) · \(Date(timeIntervalSince1970: Double(item.copiedAt) / 1000).formatted(date: .abbreviated, time: .shortened))", close: { dismiss() })
                .padding(20)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    previewContent
                    if !item.tags.isEmpty {
                        LabeledContent("标签") {
                            Text(item.tags.map { "#" + $0 }.joined(separator: "  "))
                                .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                        }.font(.callout)
                    }
                }.padding(20).frame(maxWidth: .infinity, alignment: .leading)
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
            if let error = model.errorMessage {
                NativeIssueMessage(text: error, dismiss: { model.errorMessage = nil })
                    .frame(maxHeight: 90).padding(.horizontal, 20).padding(.bottom, 12)
            }
            Divider()
            HStack(spacing: 8) {
                Text(item.available ? item.footer : "等待附件完整下载后才可粘贴")
                    .font(.callout).foregroundStyle(.secondary).lineLimit(2)
                Spacer(minLength: 8)
                Button("关闭") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("复制") { model.select(item.id); model.pasteSelection(copyOnly: true) }
                    .disabled(!item.available || model.busy)
                Button("粘贴到原窗口") { model.select(item.id); dismiss(); model.pasteSelection() }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    .disabled(!item.available || model.busy)
            }.padding(.horizontal, 20).padding(.vertical, 12)
        }.frame(width: 700, height: 510).background(Color(nsColor: .windowBackgroundColor))
    }

    @ViewBuilder private var previewContent: some View {
        if item.kind == "image" {
            LocalImage(path: item.imagePath, maxPixels: 1600)
                .frame(maxWidth: .infinity).frame(minHeight: 280).accessibilityLabel("安全图片预览")
        } else if item.kind == "files" {
            VStack(alignment: .leading, spacing: 12) {
                ForEach(Array(item.files.enumerated()), id: \.offset) { index, file in
                    HStack(alignment: .top, spacing: 12) {
                        Image(systemName: "doc").font(.title2).foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 4) {
                            Text(file.name).font(.body.weight(.medium)).textSelection(.enabled)
                                .fixedSize(horizontal: false, vertical: true)
                            Text(ByteCountFormatter.string(fromByteCount: Int64(clamping: file.size), countStyle: .file))
                                .font(.callout).foregroundStyle(.secondary)
                        }
                        Spacer(minLength: 8)
                        Label(item.available ? "可用" : "等待下载", systemImage: item.available ? "checkmark.circle" : "arrow.down.circle")
                            .font(.callout).foregroundStyle(.secondary).fixedSize()
                    }
                    if index < item.files.count - 1 { Divider() }
                }
            }
        } else {
            VStack(alignment: .leading, spacing: 12) {
                if item.kind == "rich_text" {
                    Label {
                        Text("安全纯文本预览；不执行 HTML，也不加载远程资源。原始富文本将在粘贴时保留。")
                            .fixedSize(horizontal: false, vertical: true)
                    } icon: { Image(systemName: "textformat") }
                        .font(.callout).foregroundStyle(.secondary)
                }
                Text(item.text.isEmpty ? "该内容没有纯文本回退。" : item.text)
                    .font(.system(.body, design: item.isCode ? .monospaced : .default))
                    .lineSpacing(4).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

struct TagEditor: View {
    let item: ClipItem
    @ObservedObject var model: AppModel
    @State private var input = ""
    @FocusState private var inputFocused: Bool
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(spacing: 0) {
            SheetHeader(title: "编辑标签", close: { dismiss() }).padding(20)
            Form {
                Section {
                    TextField("标签", text: $input, prompt: Text("灵感，代码，待整理"))
                        .textFieldStyle(.roundedBorder).accessibilityLabel("标签列表").focused($inputFocused)
                } footer: { Text("使用逗号分隔多个标签。标签会同步到其他设备。") }
                Section("所选内容") {
                    Text(item.title).font(.callout).lineLimit(2).help(item.title)
                }
            }.formStyle(.grouped)
            if let error = model.errorMessage {
                NativeIssueMessage(text: error).frame(maxHeight: 80).padding(.horizontal, 20).padding(.bottom, 12)
            }
            Divider()
            HStack {
                Spacer()
                Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("保存标签") { model.saveTags(item, input: input) }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(model.busy)
            }.padding(.horizontal, 20).padding(.vertical, 12)
        }.frame(width: 460, height: 340).background(Color(nsColor: .windowBackgroundColor))
            .onAppear { input = item.tags.joined(separator: "，"); inputFocused = true }
    }
}

struct CategoryEditor: View {
    @ObservedObject var model: AppModel
    let category: ClipCategory?
    @State private var name = ""
    @State private var color = DesignTokens.categoryColors[0]
    @FocusState private var nameFocused: Bool
    @Environment(\.dismiss) private var dismiss
    private let colors = DesignTokens.categoryColors
    var body: some View {
        VStack(spacing: 0) {
            SheetHeader(title: category == nil ? "新建收藏夹" : "重命名收藏夹", close: { dismiss() }).padding(20)
            Form {
                Section {
                    TextField("名称", text: $name, prompt: Text("收藏夹名称"))
                        .textFieldStyle(.roundedBorder).accessibilityLabel("收藏夹名称").focused($nameFocused)
                    if category == nil {
                        Picker("标记颜色", selection: $color) {
                            ForEach(Array(colors.enumerated()), id: \.element) { index, value in
                                Label("颜色 \(index + 1)", systemImage: "circle.fill")
                                    .foregroundStyle(Color(hex: value)).tag(value).accessibilityLabel("颜色 \(index + 1)，\(value)")
                            }
                        }
                    }
                } footer: { Text("拖入卡片，或从卡片菜单加入收藏。收藏夹会同步到其他设备。") }
            }.formStyle(.grouped)
            if let error = model.errorMessage {
                NativeIssueMessage(text: error).frame(maxHeight: 80).padding(.horizontal, 20).padding(.bottom, 12)
            }
            Divider()
            HStack {
                Spacer()
                Button("取消") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(category == nil ? "创建收藏夹" : "保存名称") { save() }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.busy)
            }.padding(.horizontal, 20).padding(.vertical, 12)
        }.frame(width: 460, height: 300).background(Color(nsColor: .windowBackgroundColor))
            .onAppear { name = category?.name ?? ""; color = category?.color ?? colors[0]; nameFocused = true }
    }
    private func save() {
        var command: [String: Any] = ["op": category == nil ? "category_create" : "category_rename", "name": name.trimmingCharacters(in: .whitespacesAndNewlines)]
        if let category { command["id"] = category.id } else { command["color"] = color }
        model.command(command, message: "收藏夹已保存") { _ in dismiss() }
    }
}

struct ConflictView: View {
    @ObservedObject var model: AppModel
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        VStack(spacing: 0) {
            SheetHeader(title: "解决同步冲突", subtitle: "并发编辑不会静默覆盖。选择一个版本，或保留所有版本。", close: { dismiss() })
                .padding(20)
            if model.conflicts.isEmpty {
                Label("目前没有待处理的冲突", systemImage: "checkmark.circle")
                    .foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                Form {
                    ForEach(Array(model.conflicts.enumerated()), id: \.element.id) { index, conflict in
                        Section {
                            Button("全部保留") { model.resolveConflict(entity: conflict.entity, keep: "all") }
                                .disabled(conflict.entity.isEmpty || model.busy)
                                .accessibilityLabel("保留冲突 \(index + 1) 的全部版本")
                        } header: { Text("冲突 \(index + 1) · \(conflict.versions.count) 个版本") }
                        ForEach(Array(conflict.versions.enumerated()), id: \.element.id) { index, version in
                            Section {
                                ConflictVersionDetails(version: version, number: index + 1, categories: model.categories, busy: model.busy) {
                                    if version.deleted {
                                        model.confirmation = .init(title: "采用删除版本？", message: "该选择会同步到其他设备，不保留这条记录的当前内容。云端旧操作历史仍可能存在。", actionTitle: "采用删除") {
                                            model.resolveConflict(entity: conflict.entity, keep: version.opID)
                                        }
                                    } else { model.resolveConflict(entity: conflict.entity, keep: version.opID) }
                                }
                            } header: { Text("版本 \(index + 1) · \(version.typeTitle)") }
                        }
                    }
                }.formStyle(.grouped)
            }
            if let error = model.errorMessage {
                NativeIssueMessage(text: error, dismiss: { model.errorMessage = nil })
                    .frame(maxHeight: 80).padding(.horizontal, 20).padding(.bottom, 12)
            }
            Divider()
            HStack { Spacer(); Button("完成") { dismiss() }.keyboardShortcut(.cancelAction) }
                .padding(.horizontal, 20).padding(.vertical, 12)
        }.frame(width: 660, height: 500).background(Color(nsColor: .windowBackgroundColor))
            .modifier(LeafConfirmation(model: model))
    }
}

private struct ConflictVersionDetails: View {
    let version: ConflictVersion
    let number: Int
    let categories: [ClipCategory]
    let busy: Bool
    let adopt: () -> Void
    var body: some View {
        HStack {
            Label(version.statusTitle, systemImage: version.deleted ? "trash" : (version.record?.symbol ?? "folder"))
                .foregroundStyle(version.deleted ? Color.red : Color.primary)
            Spacer()
            Button(version.deleted ? "采用删除…" : "保留此版", role: version.deleted ? .destructive : nil, action: adopt)
                .disabled(version.opID.isEmpty || busy)
                .accessibilityLabel(version.deleted ? "采用版本 \(number) 的删除操作" : "保留版本 \(number)")
        }
        if version.deleted {
            Text("这是明确的删除操作；采用后会同步删除，云端旧操作历史不因此彻底擦除。")
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        if let item = version.record {
            metadata("标签", value: version.tagsLabel)
            metadata("收藏夹", value: version.categoryLabel(categories))
            metadata("来源", value: item.source)
            metadata("捕获时间", value: version.capturedAtLabel)
            metadata("附件状态", value: item.available ? "可用" : "等待完整下载")
            if !item.text.isEmpty {
                DisclosureGroup("完整正文 · \(version.bodyCharacterCount.formatted()) 字符（只读）") {
                    ScrollView {
                        Text(item.text).font(.system(.body, design: item.isCode ? .monospaced : .default))
                            .lineSpacing(4).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }.frame(height: 160).padding(.top, 8)
                }
            }
            if item.kind == "image" {
                LocalImage(path: item.imagePath, maxPixels: 640).frame(height: 112).frame(maxWidth: .infinity)
                    .accessibilityLabel("版本 \(number) 的安全图片预览")
                if let image = item.image {
                    attachment(name: image.name, size: image.size, checksum: image.checksumDescription)
                } else {
                    Text("原图元信息未提供；仅凭预览无法确认字节一致。")
                        .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                }
            }
            ForEach(Array(item.files.enumerated()), id: \.offset) { _, file in
                attachment(name: file.name, size: file.size, checksum: file.checksumDescription)
            }
            if !version.formats.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("富文本格式指纹 · 不渲染或加载资源").font(.callout.weight(.medium))
                    ForEach(version.formats) { format in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(format.summary).font(.callout).foregroundStyle(.secondary)
                            if let hash = format.sha256 {
                                Text("SHA-256：" + hash).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                                    .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                            }
                        }
                    }
                }
            }
        } else if let category = version.category {
            metadata("收藏夹名", value: category.name)
            metadata("颜色值", value: category.color)
        }
        DisclosureGroup("设备与版本标识") {
            VStack(alignment: .leading, spacing: 8) {
                metadata("设备", value: version.device)
                metadata("版本", value: version.opID)
            }.padding(.top, 8)
        }
    }
    private func metadata(_ title: String, value: String) -> some View {
        LabeledContent(title) {
            Text(value).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
        }
    }
    private func attachment(name: String, size: UInt64, checksum: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Label(name, systemImage: "doc").font(.body.weight(.medium))
                .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            Text("\(ByteCountFormatter.string(fromByteCount: Int64(clamping: size), countStyle: .file)) · \(size.formatted()) 字节")
                .font(.callout).foregroundStyle(.secondary)
            Text(checksum).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct LeafConfirmation: ViewModifier {
    @ObservedObject var model: AppModel
    var active = true
    func body(content: Content) -> some View {
        content.alert(model.confirmation?.title ?? "确认操作", isPresented: Binding(get: { active && model.confirmation != nil }, set: { if !$0 && active { model.confirmation = nil } }), presenting: model.confirmation) { value in
            Button("取消", role: .cancel) { model.confirmation = nil }
            Button(value.actionTitle, role: .destructive) { value.action(); model.confirmation = nil }
        } message: { value in Text(value.message) }
    }
}
