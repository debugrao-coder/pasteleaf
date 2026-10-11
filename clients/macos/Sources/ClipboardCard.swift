import AppKit
import ImageIO
import SwiftUI

struct LocalImage: View {
    var path: String?
    var maxPixels: Int = 600
    @State private var image: NSImage?
    @State private var failed = false
    var body: some View {
        Group {
            if let image { Image(nsImage: image).resizable().aspectRatio(contentMode: .fit) }
            else { VStack(spacing: 8) { Image(systemName: failed ? "photo.badge.exclamationmark" : "photo").font(.system(size: 36, weight: .ultraLight)); if failed { Text("无法安全预览").font(.system(size: 11)) } }.foregroundStyle(.secondary) }
        }.task(id: path) {
            guard let path, !path.isEmpty else { image = nil; failed = false; return }
            let size = maxPixels
            let result = await Task.detached(priority: .utility) { () -> NSImage? in
                guard let source = try? SafeImage.previewSource(at: URL(fileURLWithPath: path)), let cgImage = CGImageSourceCreateThumbnailAtIndex(source, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceThumbnailMaxPixelSize: size, kCGImageSourceCreateThumbnailWithTransform: true] as CFDictionary) else { return nil }
                return NSImage(cgImage: cgImage, size: .zero)
            }.value
            if !Task.isCancelled { image = result; failed = result == nil }
        }
    }
}

struct ClipboardCard: View {
    let item: ClipItem
    @ObservedObject var model: AppModel
    @Environment(\.colorScheme) private var scheme
    @Environment(\.nativeAppearance) private var appearance
    @State private var hovered = false
    private var theme: LeafTheme { LeafTheme(scheme) }
    private var selected: Bool { model.selectedIDs.contains(item.id) || (model.selectedIDs.isEmpty && model.focusedID == item.id) }
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 7) {
                Image(systemName: item.symbol).font(.system(size: 12, weight: .medium)).foregroundStyle(theme.muted)
                Text(item.typeTitle).font(.system(size: 12, weight: .medium)).foregroundStyle(theme.muted)
                Spacer(minLength: 0)
                if selected { Image(systemName: "checkmark.circle.fill").font(.system(size: 13)).foregroundStyle(theme.accent).accessibilityLabel("已选择") }
                if item.category != nil { Image(systemName: "pin.fill").font(.system(size: 9)).accessibilityLabel("已收藏") }
                Menu { cardMenu } label: { Image(systemName: "ellipsis").font(.system(size: 15, weight: .medium)).frame(width: 20, height: 22) }.menuStyle(.borderlessButton).fixedSize().help("记录操作").accessibilityLabel("\(item.typeTitle)操作")
            }.foregroundStyle(theme.text).padding(.horizontal, 14).frame(height: 36).layoutPriority(1)
            content.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading).clipped()
                .padding(.horizontal, 14).padding(.vertical, 8)
            // Metadata is outside the flexible preview, so compact cards never crop it.
            VStack(alignment: .leading, spacing: 4) {
                if !item.tags.isEmpty {
                    Text(item.tags.map { "#" + $0 }.joined(separator: "  "))
                        .font(.system(size: 10, weight: .medium)).foregroundStyle(theme.muted).lineLimit(1)
                        .frame(height: 16, alignment: .leading).help(item.tags.joined(separator: "，"))
                }
                HStack(spacing: 6) {
                    if !item.available { Image(systemName: "arrow.down.circle").foregroundStyle(theme.warning) }
                    Text(item.source.isEmpty ? "未知来源" : item.source).lineLimit(1)
                    Spacer(minLength: 4)
                    Text(relativeTime).lineLimit(1).fixedSize(horizontal: true, vertical: false)
                }.font(.system(size: 10)).foregroundStyle(theme.muted).frame(height: 16)
            }.padding(.horizontal, 14).padding(.bottom, 8).fixedSize(horizontal: false, vertical: true).layoutPriority(1)
        }
        .frame(width: DesignTokens.cardWidth, height: DesignTokens.cardHeight)
        .background(theme.surface)
        .clipShape(RoundedRectangle(cornerRadius: DesignTokens.cardRadius))
        .overlay(RoundedRectangle(cornerRadius: DesignTokens.cardRadius).strokeBorder(selected ? theme.accent : (hovered ? theme.muted.opacity(0.6) : theme.border), lineWidth: selected || appearance.increaseContrast ? 2 : 0.5))
        .shadow(color: .black.opacity(appearance.increaseContrast ? 0 : (scheme == .dark ? 0.16 : 0.06)), radius: hovered ? 4 : 2, x: 0, y: 1)
        .contentShape(RoundedRectangle(cornerRadius: DesignTokens.cardRadius))
        .onHover { hovered = $0 }
        .onTapGesture(count: 2) { model.select(item.id); model.pasteSelection() }
        .onTapGesture { model.select(item.id, modifiers: NSEvent.modifierFlags) }
        .contextMenu { cardMenu }
        .draggable(item.id) { Text(item.title).padding(10).background(theme.surface, in: RoundedRectangle(cornerRadius: DesignTokens.controlRadius)).frame(maxWidth: DesignTokens.cardWidth) }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(item.typeTitle)，\(item.title)，来源 \(item.source)，\(relativeTime)，\(item.tags.isEmpty ? "无标签" : item.tags.joined(separator: "，"))，\(item.available ? "可用" : "等待附件")")
        .accessibilityAddTraits(selected ? [.isSelected, .isButton] : [.isButton])
        .accessibilityAction(named: "选择") { model.select(item.id) }
        .accessibilityAction(named: "预览") { model.requestPreview(item) }
        .accessibilityAction(named: "复制") { model.select(item.id); model.pasteSelection(copyOnly: true) }
    }
    @ViewBuilder private var content: some View {
        if item.kind == "image" {
            LocalImage(path: item.imagePath).frame(maxWidth: .infinity, maxHeight: .infinity).background(theme.elevated, in: RoundedRectangle(cornerRadius: DesignTokens.controlRadius))
        } else if item.kind == "files" {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 10) {
                    Image(systemName: "doc.fill").font(.system(size: 32, weight: .regular)).foregroundStyle(theme.accent)
                    Text(item.files.first?.name ?? "等待文件").font(.system(size: 13, weight: .medium)).foregroundStyle(theme.text).lineLimit(2)
                }
                Text(item.footer).font(.system(size: 11)).foregroundStyle(item.available ? theme.muted : theme.warning).lineLimit(1)
            }
        } else if item.isLink {
            VStack(alignment: .leading, spacing: 4) {
                Image(systemName: "globe").font(.system(size: 24, weight: .light)).foregroundStyle(theme.accent)
                Text(URL(string: item.text)?.host ?? "链接").font(.system(size: 15, weight: .semibold)).lineLimit(1).foregroundStyle(theme.text)
                Text(item.text).font(.system(size: 11)).foregroundStyle(theme.muted).lineLimit(2)
            }
        } else {
            Text(item.text.isEmpty ? "富文本内容（无纯文本预览）" : item.text)
                    .font(.system(size: item.isCode ? 11 : 13, weight: .regular, design: item.isCode ? .monospaced : .default))
                    .lineSpacing(item.isCode ? 3 : 5).foregroundStyle(theme.text).lineLimit(item.isCode ? 5 : 4)
                    .frame(maxWidth: .infinity, alignment: .topLeading)
        }
    }
    @ViewBuilder private var cardMenu: some View {
        Button("预览", systemImage: "eye") { model.requestPreview(item) }
        Button("复制", systemImage: "doc.on.doc") { model.select(item.id); model.pasteSelection(copyOnly: true) }.disabled(!item.available)
        Button("粘贴", systemImage: "arrow.turn.down.left") { model.select(item.id); model.pasteSelection() }.disabled(!item.available)
        Button("粘贴为纯文本", systemImage: "textformat") { model.select(item.id); model.pasteSelection(plain: true) }.disabled(item.text.isEmpty)
        Divider()
        Menu("收藏到", systemImage: "pin") {
            ForEach(model.categories) { category in Button(category.name) { model.setFavorite(item, category: category.id) } }
            if model.categories.isEmpty { Button("先创建收藏夹") { model.showCategoryEditor = true } }
        }
        if item.category != nil { Button("移出收藏", systemImage: "pin.slash") { model.setFavorite(item, category: nil) } }
        Button("编辑标签", systemImage: "tag") { model.editTags = item }
        Divider()
        Button("删除记录…", systemImage: "trash", role: .destructive) { model.delete(item) }
    }
    private var relativeTime: String {
        let interval = max(0, Int(Date().timeIntervalSince1970 - Double(item.copiedAt) / 1000))
        if interval < 60 { return "刚刚" }; if interval < 3600 { return "\(interval / 60) 分钟前" }; if interval < 86400 { return "\(interval / 3600) 小时前" }; return "\(interval / 86400) 天前"
    }
}
