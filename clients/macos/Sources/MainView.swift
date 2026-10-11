import AppKit
import SwiftUI

extension Notification.Name { static let focusPasteLeafSearch = Notification.Name("io.pasteleaf.focusSearch") }

struct NativeSearchField: NSViewRepresentable {
    @ObservedObject var model: AppModel
    var onCommit: () -> Void
    func makeCoordinator() -> Coordinator { Coordinator(model: model, onCommit: onCommit) }
    func makeNSView(context: Context) -> NSSearchField {
        let field = NSSearchField(); field.placeholderString = "搜索内容、标签或来源"; field.font = .systemFont(ofSize: 13)
        field.controlSize = .regular; field.focusRingType = .exterior; field.delegate = context.coordinator
        field.sendsSearchStringImmediately = false; field.sendsWholeSearchString = false
        field.setAccessibilityLabel("搜索剪贴板历史")
        context.coordinator.field = field
        context.coordinator.observer = NotificationCenter.default.addObserver(forName: .focusPasteLeafSearch, object: nil, queue: .main) { [weak field] _ in field?.window?.makeFirstResponder(field) }
        return field
    }
    func updateNSView(_ view: NSSearchField, context: Context) {
        if view.stringValue != model.query, (view.currentEditor() as? NSTextView)?.hasMarkedText() != true { view.stringValue = model.query }
    }
    final class Coordinator: NSObject, NSSearchFieldDelegate {
        var model: AppModel; let onCommit: () -> Void; weak var field: NSSearchField?; var observer: NSObjectProtocol?
        init(model: AppModel, onCommit: @escaping () -> Void) { self.model = model; self.onCommit = onCommit }
        @MainActor func controlTextDidChange(_ obj: Notification) {
            guard let field = obj.object as? NSSearchField, (field.currentEditor() as? NSTextView)?.hasMarkedText() != true else { return }
            model.searchChanged(field.stringValue)
        }
        @MainActor func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            guard !textView.hasMarkedText() else { return false }
            if commandSelector == #selector(NSResponder.insertNewline(_:)) || commandSelector == #selector(NSResponder.moveDown(_:)) {
                onCommit(); return true
            }
            if commandSelector == #selector(NSResponder.cancelOperation(_:)) {
                if !model.query.isEmpty { model.searchChanged("") } else { onCommit() }
                return true
            }
            return false
        }
        deinit { if let observer { NotificationCenter.default.removeObserver(observer) } }
    }
}

struct MainView: View {
    @ObservedObject var model: AppModel
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    private var theme: LeafTheme { LeafTheme(scheme) }
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.accessibilityShowBorders) private var showBorders
    private var appearance: NativeAppearancePolicy {
        NativeAppearancePolicy(
            macOSMajorVersion: ProcessInfo.processInfo.operatingSystemVersion.majorVersion,
            appReduceTransparency: model.settings["reduce_transparency"] as? Bool == true,
            systemReduceTransparency: reduceTransparency,
            systemIncreaseContrast: contrast == .increased,
            appReduceMotion: model.settings["reduce_motion"] as? Bool == true,
            systemReduceMotion: reduceMotion,
            systemShowButtonShapes: showBorders)
    }
    var body: some View {
        GeometryReader { geometry in
            let shelf = shelf(availableWidth: PanelLayout.contentWidth(for: geometry.size.width))
                .frame(width: geometry.size.width, height: PanelLayout.contentHeight)
            if PanelLayout.needsVerticalScrolling(availableHeight: geometry.size.height) {
                ScrollView(.vertical) { shelf }
            } else { shelf }
        }
        .background {
            ZStack {
                VisualBackdrop(disabled: appearance.reduceTransparency, scheme: scheme)
                if appearance.reduceTransparency { theme.background }
            }
        }
        .foregroundStyle(theme.text)
        .clipShape(RoundedRectangle(cornerRadius: DesignTokens.panelRadius))
        .overlay(RoundedRectangle(cornerRadius: DesignTokens.panelRadius).strokeBorder(theme.border, lineWidth: appearance.increaseContrast ? 2 : 0.5))
        .preferredColorScheme(model.appearance == "dark" ? .dark : (model.appearance == "light" ? .light : nil))
        .sheet(item: $model.preview) { item in PreviewView(item: item, model: model) }
        .sheet(item: $model.editTags) { item in TagEditor(item: item, model: model) }
        .sheet(isPresented: $model.showCategoryEditor) { CategoryEditor(model: model, category: model.categoryEditor) }
        .sheet(isPresented: $model.showConflicts) { ConflictView(model: model) }
        .modifier(LeafConfirmation(model: model, active: !model.showSettings && !model.showConflicts))
        .environment(\.nativeAppearance, appearance)
        .transaction { if !appearance.allowsAnimations { $0.disablesAnimations = true; $0.animation = nil } }
    }
    private func shelf(availableWidth: CGFloat) -> some View {
        VStack(spacing: 0) {
            header(compact: availableWidth < 760)
            if !model.hasLoaded && model.items.isEmpty {
                ScrollView(.vertical) { VStack(spacing: 14) {
                    if let error = model.errorMessage {
                        Image(systemName: "externaldrive.badge.exclamationmark").font(.system(size: 30, weight: .light)).foregroundStyle(theme.warning)
                        Text("本地资料库未能打开").font(.system(size: 17, weight: .semibold))
                        Text(error).font(.system(size: 12)).foregroundStyle(theme.muted).multilineTextAlignment(.center).frame(maxWidth: 580)
                        Button("重试") { model.errorMessage = nil; model.refreshStatus { model.reload(force: true) } }.buttonStyle(LeafButtonStyle())
                    } else { ProgressView("正在打开本地资料库…") }
                }.frame(maxWidth: .infinity).padding(.vertical, 12) }.frame(maxHeight: .infinity)
            }
            else if !model.onboardingComplete {
                // Consent and its buttons stay reachable at compact heights or narrow widths.
                ScrollView(.vertical) { OnboardingView(model: model, compact: availableWidth < 720).frame(maxWidth: .infinity) }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            else {
                categoryBar(compact: availableWidth < 640)
                cardStrip
                footer(compact: availableWidth < 520)
            }
        }
        .padding(.horizontal, DesignTokens.panelPadding)
        .padding(.top, DesignTokens.panelTopPadding).padding(.bottom, DesignTokens.panelBottomPadding)
    }
    private func header(compact: Bool) -> some View {
        NativeControlGroup { HStack(spacing: compact ? 8 : 12) {
            HStack(spacing: 8) {
                Image(systemName: "leaf.fill").font(.system(size: 17, weight: .medium)).foregroundStyle(theme.muted)
                if !compact {
                    Text("PasteLeaf").font(.system(size: 14, weight: .semibold))
                    if model.isSyntheticSession { Text(model.isDemo ? "演示" : "隔离验证").font(.system(size: 10, weight: .semibold)).foregroundStyle(theme.muted).padding(.horizontal, 6).padding(.vertical, 3).background(theme.surface, in: Capsule()) }
                }
            }.fixedSize().accessibilityElement(children: .combine).accessibilityLabel(model.isSyntheticSession ? "PasteLeaf · 合成数据验证" : "PasteLeaf")
            if !compact { Spacer(minLength: 16) }
            NativeSearchField(model: model) { NSApp.keyWindow?.makeFirstResponder(nil); if model.focusedID == nil { model.focusedID = model.items.first?.id } }
                .frame(minWidth: compact ? 72 : 220, idealWidth: compact ? 180 : 310, maxWidth: compact ? .infinity : 360).frame(height: 30).disabled(!model.onboardingComplete)
            if compact {
                IconButton(symbol: model.syncSymbol, title: model.syncTitle + "：" + model.syncDetail) {
                    if model.conflictCount > 0 { model.loadConflicts() } else { model.settingsTab = "sync"; model.showSettings = true }
                }
                Menu {
                    Button("设置…") { model.showSettings = true }
                    Button(model.paused ? "恢复记录" : "暂停记录") { model.setPaused(!model.paused) }.disabled(model.isSyntheticSession || !model.onboardingComplete)
                } label: { Image(systemName: "slider.horizontal.3").font(.system(size: 15)).frame(width: 30, height: 30) }
                    .menuStyle(.borderlessButton).fixedSize().accessibilityLabel("设置与记录选项")
            } else {
                StatusPill(model: model)
                IconButton(symbol: model.paused ? "pause.circle" : "record.circle", title: model.paused ? "恢复记录" : "暂停记录") {
                    if !model.onboardingComplete { return }; model.setPaused(!model.paused)
                }.disabled(model.isSyntheticSession || !model.onboardingComplete)
                IconButton(symbol: "slider.horizontal.3", title: "设置（⌘,）") { model.showSettings = true }
            }
            IconButton(symbol: "xmark", title: "收起 PasteLeaf（Esc）") { model.onHide?() }
        } }.frame(height: DesignTokens.headerHeight).padding(.bottom, DesignTokens.headerGap)
    }
    @ViewBuilder private func categoryBar(compact: Bool) -> some View {
        Group {
            if compact { ScrollView(.horizontal, showsIndicators: true) { categoryNavigation(compact: true).fixedSize(horizontal: true, vertical: false) } }
            else { categoryNavigation(compact: false) }
        }.frame(height: DesignTokens.categoryHeight).padding(.bottom, DesignTokens.categoryGap)
    }
    private func categoryNavigation(compact: Bool) -> some View {
        NativeControlGroup { HStack(spacing: 12) {
            categoryButton("历史", symbol: "clock", id: nil)
            categoryButton("全部收藏", symbol: "pin", id: "favorites")
            Rectangle().fill(theme.border).frame(width: 1, height: 16)
            Group {
                if compact { categoryChips }
                else { ScrollView(.horizontal, showsIndicators: false) { categoryChips } }
            }.frame(height: 30)
            IconButton(symbol: "plus", title: "新建收藏夹") { model.categoryEditor = nil; model.showCategoryEditor = true }
            if model.newItemsWaiting > 0 {
                Button { model.showNewItems() } label: { Label("\(model.newItemsWaiting) 条新记录", systemImage: "arrow.up").font(.system(size: 11, weight: .medium)) }.buttonStyle(.bordered).controlSize(.small)
            } else { Text("\(model.total.formatted()) 条记录").font(.system(size: 11)).monospacedDigit().foregroundStyle(theme.muted).lineLimit(1) }
        } }
    }
    private var categoryChips: some View {
        HStack(spacing: 6) {
                    ForEach(model.categories) { category in
                        categoryButton(category.name, symbol: "circle.fill", id: category.id, tint: Color(hex: category.color))
                            .contextMenu {
                                Button("重命名收藏夹") { model.categoryEditor = category; model.showCategoryEditor = true }
                                Button("删除收藏夹…", role: .destructive) {
                                    model.confirmation = .init(title: "删除“\(category.name)”？", message: "收藏夹会从所有设备移除，记录仍保留在历史中。", actionTitle: "删除收藏夹") { model.command(["op": "category_delete", "id": category.id], message: "收藏夹已删除"); if model.category == category.id { model.chooseCategory(nil) } }
                                }
                            }
                            .dropDestination(for: String.self) { ids, _ in
                                for id in ids { if let item = model.items.first(where: { $0.id == id }) { model.setFavorite(item, category: category.id) } }
                                return !ids.isEmpty
                            }
                    }
        }
    }
    private func categoryButton(_ title: String, symbol: String, id: String?, tint: Color? = nil) -> some View {
        Button { model.chooseCategory(id) } label: {
            HStack(spacing: 6) {
                Image(systemName: symbol).font(.system(size: symbol == "circle.fill" ? 6 : 11, weight: .medium))
                    .foregroundStyle(model.category == id ? theme.accentText : (tint ?? theme.muted))
                Text(title).lineLimit(1).font(.system(size: 12, weight: model.category == id ? .semibold : .regular))
            }
        }.buttonStyle(NativeToolbarButtonStyle(selected: model.category == id)).buttonBorderShape(.capsule).accessibilityAddTraits(model.category == id ? .isSelected : []).help(title)
    }
    @ViewBuilder private var cardStrip: some View {
        if model.items.isEmpty {
            VStack(spacing: 10) {
                Image(systemName: model.query.isEmpty ? "tray" : "magnifyingglass").font(.system(size: 32, weight: .ultraLight)).foregroundStyle(theme.accent)
                Text(model.query.isEmpty ? (model.category == nil ? "值得保留的，从下一次复制开始" : "给这个收藏夹留一些好东西") : "没有找到匹配内容").font(.system(size: 16, weight: .medium))
                Text(model.query.isEmpty ? (model.paused ? "记录已暂停。你可以在准备好时恢复。" : "复制文字、链接、图片或文件，PasteLeaf 会在这里收好。") : "换个关键词，或清除搜索再试一次。")
                    .font(.system(size: 12)).foregroundStyle(theme.muted)
                if !model.query.isEmpty { Button("清除搜索") { model.searchChanged("") }.buttonStyle(LeafButtonStyle()) }
            }.frame(maxWidth: .infinity).frame(height: DesignTokens.cardHeight + DesignTokens.cardStripExtra)
        } else {
            ScrollViewReader { proxy in
                ScrollView(.horizontal, showsIndicators: true) {
                    LazyHStack(spacing: DesignTokens.gap) {
                        ForEach(model.items) { item in ClipboardCard(item: item, model: model).id(item.id) }
                        if model.items.count < model.total {
                            Button { model.loadMore() } label: { VStack(spacing: 10) { Image(systemName: "arrow.right.circle").font(.system(size: 24, weight: .light)); Text(model.isLoading ? "正在加载…" : "载入更多").font(.system(size: 12)); Text("已载入 \(model.items.count)").font(.system(size: 10)) }.frame(width: 100, height: DesignTokens.cardHeight) }
                                .buttonStyle(.plain).foregroundStyle(theme.muted).disabled(model.isLoading)
                        }
                    }.padding(.horizontal, 2).padding(.vertical, 3)
                }.frame(height: DesignTokens.cardHeight + DesignTokens.cardStripExtra)
                    .onChange(of: model.focusedID) { _, id in
                        guard let id else { return }
                        if !appearance.allowsAnimations { proxy.scrollTo(id) }
                        else { withAnimation(.easeOut(duration: DesignTokens.motionSeconds)) { proxy.scrollTo(id) } }
                    }
            }
        }
    }
    private func footer(compact: Bool) -> some View {
        HStack(spacing: 8) {
            if !compact || model.sequence.isEmpty || model.errorMessage != nil { footerFeedback(compact: compact).frame(maxWidth: .infinity, alignment: .leading).layoutPriority(-1) }
            Spacer(minLength: 0)
            if !model.sequence.isEmpty {
                ViewThatFits(in: .horizontal) {
                    queueControls(compact: false).fixedSize(horizontal: true, vertical: false)
                    queueControls(compact: true).fixedSize(horizontal: true, vertical: false)
                }.layoutPriority(1)
            } else if !compact {
                Text(model.isSyntheticSession ? (model.isDemo ? "合成数据 · 不访问真实剪贴板" : "隔离验证 · 不访问剪贴板或云端") : (model.paused ? "记录已暂停" : "仅本机记录 · 加密同步按设置启用"))
                    .font(.system(size: 10)).foregroundStyle(theme.muted).lineLimit(1)
                Keycap(text: model.hotkeyDisplay).fixedSize()
            }
        }.frame(height: DesignTokens.footerHeight).padding(.top, DesignTokens.footerGap)
    }
    private func footerFeedback(compact: Bool) -> some View {
        HStack(spacing: 8) {
            if let error = model.errorMessage {
                Label(error, systemImage: "exclamationmark.circle").font(.system(size: 11)).foregroundStyle(theme.danger).lineLimit(1).help(error)
                IconButton(symbol: "xmark.circle", title: "关闭错误提示") { model.errorMessage = nil }
            } else if let notice = model.notice {
                Text(notice).font(.system(size: 11)).foregroundStyle(theme.text).lineLimit(1)
            } else if model.selectedIDs.count > 1 {
                Text("已选择 \(model.selectedIDs.count) 项").font(.system(size: 11)).foregroundStyle(theme.text)
                Button("复制") { model.pasteSelection(copyOnly: true) }.buttonStyle(.bordered).controlSize(.small).font(.system(size: 11))
                Button("纯文本粘贴") { model.pasteSelection(plain: true) }.buttonStyle(.bordered).controlSize(.small).font(.system(size: 11))
            } else if compact {
                Text(model.isSyntheticSession ? (model.isDemo ? "合成演示 · 不访问剪贴板" : "隔离验证 · 不访问剪贴板或云端") : (model.paused ? "记录已暂停" : "⌘F 搜索 · 空格预览 · ↵ 粘贴"))
                    .font(.system(size: 10)).foregroundStyle(theme.muted).lineLimit(1)
            } else {
                HStack(spacing: 5) { Keycap(text: "← →"); Text("选择"); Keycap(text: "空格"); Text("预览"); Keycap(text: "↵"); Text("粘贴") }.font(.system(size: 10)).foregroundStyle(theme.muted)
            }
        }
    }
    private func queueControls(compact: Bool) -> some View {
        HStack(spacing: 8) {
            Button {
                model.pasteNext()
            } label: {
                Label(compact ? "\(model.sequenceIndex + 1)/\(model.sequence.count)" : "粘贴下一项 \(model.sequenceIndex + 1)/\(model.sequence.count)", systemImage: "arrow.turn.down.left")
                    .lineLimit(1)
            }.buttonStyle(LeafButtonStyle(prominent: true)).disabled(model.sequencePreparing)
                .accessibilityLabel("粘贴下一项，第 \(model.sequenceIndex + 1) 项，共 \(model.sequence.count) 项")
            IconButton(symbol: "xmark.circle", title: "取消顺序队列") { model.sequence = []; model.sequenceIndex = 0 }
        }
    }
}

struct OnboardingView: View {
    @ObservedObject var model: AppModel
    var compact = false
    @State private var consent = false
    @Environment(\.colorScheme) private var scheme
    var body: some View {
        Group {
            if compact { VStack(alignment: .leading, spacing: 20) { introduction; consentControls } }
            else { HStack(spacing: 32) { introduction; consentControls } }
        }.padding(.horizontal, compact ? 0 : 28).padding(.vertical, 12)
    }
    private var introduction: some View {
        VStack(alignment: .leading, spacing: 12) {
                Label("设置 PasteLeaf", systemImage: "doc.on.clipboard").font(.system(size: 21, weight: .semibold))
                Text("在本机保存剪贴板历史，随时查找和再次粘贴。\n需要跨设备使用时，再连接 iCloud。").font(.system(size: 13)).foregroundStyle(LeafTheme(scheme).muted).lineSpacing(5)
                Label("应用不会自动导入或修改 Paste", systemImage: "checkmark.shield").font(.system(size: 11)).foregroundStyle(LeafTheme(scheme).muted)
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
    private var consentControls: some View {
        VStack(alignment: .leading, spacing: 12) {
                InfoBanner(text: "开启后，文字、图片、富文本和普通文件会自动记录；密码、Token 等敏感内容也可能被保存。本地正文不加密。可随时暂停或排除应用。", symbol: "hand.raised", warning: true)
                Toggle("我已了解记录范围，并愿意开启", isOn: $consent).toggleStyle(.checkbox).font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
                HStack {
                    Button("先不记录") { model.patchSettings(["onboarding_complete": true, "paused": true]) }.buttonStyle(LeafButtonStyle())
                    Spacer()
                    Button("开启本机记录") { model.enableRecording() }.buttonStyle(LeafButtonStyle(prominent: true)).disabled(!consent || model.busy)
                }
                Text("同步尚未启用。不会自动连接 iCloud 或覆盖其他设备的系统剪贴板。").font(.system(size: 10)).foregroundStyle(LeafTheme(scheme).muted).fixedSize(horizontal: false, vertical: true)
        }.frame(maxWidth: .infinity)
    }
}
