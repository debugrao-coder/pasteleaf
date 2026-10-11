import AppKit
import Combine
import Foundation

@MainActor
final class AppModel: ObservableObject {
    let engine: ClipboardEngine
    let systemIntegrationAllowed: Bool
    @Published var items: [ClipItem] = []
    @Published var total = 0
    @Published var query = ""
    @Published var category: String?
    @Published var categories: [ClipCategory] = []
    @Published var selectedIDs: Set<String> = []
    @Published var focusedID: String?
    @Published var settings: [String: Any] = [:]
    @Published var syncState = "disconnected"
    @Published var syncDetail = "尚未连接 iCloud Drive"
    @Published var syncDirectory = ""
    @Published var pending = 0
    @Published var conflictCount = 0
    @Published var peerReceipts = 0
    @Published var lastExchange: Date?
    @Published var isLoading = false
    @Published var busyCount = 0
    @Published var hasLoaded = false
    @Published var errorMessage: String?
    @Published var notice: String?
    @Published var newItemsWaiting = 0
    @Published var preview: ClipItem?
    @Published var showSettings = false
    @Published var settingsTab = "general"
    @Published var editTags: ClipItem?
    @Published var categoryEditor: ClipCategory?
    @Published var showCategoryEditor = false
    @Published var confirmation: Confirmation?
    @Published var sequence: [ClipItem] = []
    @Published var sequenceIndex = 0
    @Published var sequencePreparing = false
    @Published var conflicts: [ConflictGroup] = []
    @Published var showConflicts = false
    @Published private(set) var hotkeyEditor = HotkeyRecordingState()
    @Published private(set) var isApplyingHotkey = false
    @Published private(set) var hotkeyError: String?
    @Published private(set) var hotkeyNotice: String?
    var onSettingsChanged: (() -> Void)?
    var onRegisterHotkey: ((String) throws -> Void)?
    var onHotkeyRegistrationFinished: ((Bool) -> Void)?
    var onHotkeyRecordingChanged: ((Bool) -> Void)?
    var onShowPanel: (() -> Void)?
    var onPaste: ((PasteBatch, Bool) -> Bool)?
    var onHide: (() -> Void)?
    var onPermissionRequest: (() -> Void)?
    var onLoginChange: ((Bool) -> Void)?
    var isPanelVisible = false
    private var requestGeneration = 0
    private var pendingPage: ClipPage?
    private var searchWork: DispatchWorkItem?
    private var refreshTimer: Timer?
    private var statusInFlight = false
    private var lastSelectionAnchor: String?
    private var conflictGeneration = 0
    private var hotkeySettingsRevision = 0

    struct Confirmation: Identifiable {
        let id = UUID(); let title: String; let message: String; let actionTitle: String; let action: () -> Void
    }
    init(engine: ClipboardEngine, systemIntegrationAllowed: Bool = true) { self.engine = engine; self.systemIntegrationAllowed = systemIntegrationAllowed }
    var isDemo: Bool { engine.isDemo }
    var isTestLibrary: Bool { !isDemo && !systemIntegrationAllowed }
    var isSyntheticSession: Bool { isDemo || isTestLibrary }
    var busy: Bool { busyCount > 0 }
    var onboardingComplete: Bool { isDemo || settings["onboarding_complete"] as? Bool == true }
    var paused: Bool { !onboardingComplete || settings["paused"] as? Bool != false }
    var appearance: String { settings["appearance"] as? String ?? "system" }
    var hotkey: String { settings["hotkey"] as? String ?? HotkeySpec.defaultValue }
    var isRecordingHotkey: Bool { hotkeyEditor.isRecording }
    var excludedApps: [String] { settings["excluded_apps"] as? [String] ?? [] }
    var selectedItems: [ClipItem] { items.filter { selectedIDs.contains($0.id) } }
    var focusedItem: ClipItem? { items.first { $0.id == focusedID } }
    var activeItems: [ClipItem] { selectedItems.isEmpty ? focusedItem.map { [$0] } ?? [] : selectedItems }
    var hotkeyDisplay: String { (try? HotkeySpec.parse(hotkey).display) ?? "⌥⌘V" }
    var syncTitle: String {
        if isDemo { return "安全演示" }
        if isTestLibrary { return "隔离验证" }
        switch syncState {
        case "locked": return "同步已锁定"
        case "paused": return "同步已暂停"
        case "pending": return "\(pending) 项待发布"
        case "published": return "已写入同步目录"
        case "waiting_attachments": return "等待附件"
        case "peer_imported": return "对端已导入"
        case "conflict": return "\(conflictCount) 个冲突"
        case "error": return "同步需要处理"
        default: return "连接 iCloud"
        }
    }
    var syncSymbol: String {
        switch syncState { case "error", "conflict": return "exclamationmark.icloud"; case "locked": return "lock.icloud"; case "paused": return "pause.circle"; case "peer_imported": return "checkmark.icloud"; default: return "icloud" }
    }
    func start() {
        refreshStatus { [weak self] in self?.reload(force: true) }
        if !isDemo {
            refreshTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
                Task { @MainActor in self?.refreshStatus(); self?.reload(force: false, background: true) }
            }
        }
    }
    func stop() { cancelHotkeyRecording(); refreshTimer?.invalidate(); refreshTimer = nil; searchWork?.cancel() }
    func refreshStatus(completion: (() -> Void)? = nil) {
        guard !statusInFlight else { completion?(); return }
        statusInFlight = true
        let hotkeyRevision = hotkeySettingsRevision
        engine.request(["op": "status"]) { [weak self] result in
            guard let self else { return }
            self.statusInFlight = false
            switch result {
            case .success(let value):
                guard let status = value as? [String: Any] else { self.fail(CoreError(code: "invalid_status", message: "无法读取核心状态。")); break }
                if var values = status["settings"] as? [String: Any] {
                    // A queued status response must not undo a newer shortcut transaction.
                    if self.isApplyingHotkey || hotkeyRevision != self.hotkeySettingsRevision { values["hotkey"] = self.hotkey }
                    self.settings = values
                }
                self.categories = (try? decodeCore(status["categories"] ?? [], as: [ClipCategory].self)) ?? []
                self.syncState = status["sync_state"] as? String ?? "disconnected"
                self.syncDetail = SyncStatusCopy.detail(state: self.syncState, raw: status["detail"] as? String ?? "", hasError: status["error"] as? [String: Any] != nil, isDemo: self.isDemo, isTestLibrary: self.isTestLibrary)
                self.syncDirectory = status["sync_directory"] as? String ?? ""
                self.pending = status["pending"] as? Int ?? 0
                self.conflictCount = status["conflicts"] as? Int ?? 0
                self.peerReceipts = status["peer_receipts"] as? Int ?? (status["peer_receipts"] as? [Any])?.count ?? 0
                if let ms = status["last_exchange_ms"] as? Double, ms > 0 { self.lastExchange = Date(timeIntervalSince1970: ms / 1000) }
                self.hasLoaded = true
                self.onSettingsChanged?()
            case .failure(let error): self.fail(error)
            }
            completion?()
        }
    }
    func searchChanged(_ text: String) {
        query = text
        searchWork?.cancel()
        // Invalidate any currently running response immediately, before the debounce fires.
        requestGeneration += 1
        let work = DispatchWorkItem { [weak self] in self?.reload(force: true, resetSelection: true) }
        searchWork = work
        if text.isEmpty { work.perform() } else { DispatchQueue.main.asyncAfter(deadline: .now() + 0.24, execute: work) }
    }
    func chooseCategory(_ value: String?) { category = value; reload(force: true, resetSelection: true) }
    func reload(force: Bool = false, resetSelection: Bool = false, background: Bool = false) {
        requestGeneration += 1
        let generation = requestGeneration
        if !background { isLoading = true }
        let targetCount = max(items.count, 50)
        let searchQuery = query
        let searchCategory = category
        func finish(_ page: ClipPage) {
            guard generation == self.requestGeneration else { return }
            self.isLoading = false
            if !force && self.isPanelVisible && !self.items.isEmpty {
                let existing = Set(self.items.map(\.id))
                let new = page.items.filter { !existing.contains($0.id) }
                if !new.isEmpty {
                    self.pendingPage = page; self.newItemsWaiting = new.count
                    let replacements = Dictionary(uniqueKeysWithValues: page.items.map { ($0.id, $0) })
                    self.items = self.items.map { replacements[$0.id] ?? $0 }
                    self.total = page.total
                    return
                }
            }
            self.apply(page, resetSelection: resetSelection)
        }
        func loadChunk(_ offset: Int, accumulated: [ClipItem]) {
            guard generation == self.requestGeneration else { return }
            // The core caps every page at 500. Refresh all loaded pages without truncation.
            let limit = min(500, targetCount - offset)
            let command: [String: Any] = ["op": "list", "query": searchQuery, "category": searchCategory as Any? ?? NSNull(), "offset": offset, "limit": limit]
            self.engine.request(command) { [weak self] result in
                guard let self, generation == self.requestGeneration else { return }
                switch result {
                case .success(let value):
                    do {
                        let page = try decodeCore(value, as: ClipPage.self)
                        let combined = accumulated + page.items
                        let next = offset + page.items.count
                        if !page.items.isEmpty && next < min(page.total, targetCount) { loadChunk(next, accumulated: combined) }
                        else { finish(ClipPage(items: combined, total: page.total)) }
                    } catch { self.isLoading = false; self.fail(error) }
                case .failure(let error): self.isLoading = false; if !background { self.fail(error) }
                }
            }
        }
        loadChunk(0, accumulated: [])
    }
    private func apply(_ page: ClipPage, resetSelection: Bool) {
        items = page.items; total = page.total; pendingPage = nil; newItemsWaiting = 0
        let ids = Set(items.map(\.id))
        if resetSelection { selectedIDs = []; focusedID = items.first?.id; lastSelectionAnchor = focusedID }
        else {
            selectedIDs.formIntersection(ids)
            if focusedID == nil || !ids.contains(focusedID!) { focusedID = items.first?.id }
            if lastSelectionAnchor == nil || !ids.contains(lastSelectionAnchor!) { lastSelectionAnchor = focusedID }
        }
    }
    func showNewItems() { if let page = pendingPage { apply(page, resetSelection: false) } else { reload(force: true) } }
    func loadMore() {
        guard items.count < total, !isLoading else { return }
        isLoading = true
        let generation = requestGeneration
        engine.request(["op": "list", "query": query, "category": category as Any? ?? NSNull(), "offset": items.count, "limit": 50]) { [weak self] result in
            guard let self, generation == self.requestGeneration else { return }
            self.isLoading = false
            switch result {
            case .success(let value):
                do { let page = try decodeCore(value, as: ClipPage.self); let ids = Set(self.items.map(\.id)); self.items.append(contentsOf: page.items.filter { !ids.contains($0.id) }); self.total = page.total }
                catch { self.fail(error) }
            case .failure(let error): self.fail(error)
            }
        }
    }
    func select(_ id: String, modifiers: NSEvent.ModifierFlags = []) {
        // A focused first card is already the user's starting point even before a click.
        // Establish a missing/stale anchor before changing focus, never during a valid range.
        if modifiers.contains(.shift), lastSelectionAnchor == nil || !items.contains(where: { $0.id == lastSelectionAnchor }) {
            lastSelectionAnchor = items.first(where: { $0.id == focusedID })?.id ?? id
        }
        focusedID = id
        if modifiers.contains(.shift), let anchor = lastSelectionAnchor, let start = items.firstIndex(where: { $0.id == anchor }), let end = items.firstIndex(where: { $0.id == id }) {
            selectedIDs = Set(items[min(start, end)...max(start, end)].map(\.id))
        } else if modifiers.contains(.command) {
            if selectedIDs.contains(id) { selectedIDs.remove(id) } else { selectedIDs.insert(id) }
            lastSelectionAnchor = id
        } else { selectedIDs = [id]; lastSelectionAnchor = id }
    }
    func moveSelection(_ delta: Int, extend: Bool = false) {
        guard !items.isEmpty else { return }
        let old = items.firstIndex { $0.id == focusedID } ?? 0
        let next = min(max(0, old + delta), items.count - 1)
        select(items[next].id, modifiers: extend ? [.shift] : [])
    }
    func selectAll() { selectedIDs = Set(items.map(\.id)); announce("已选择当前载入的 \(items.count) 条记录") }
    func command(_ command: [String: Any], message: String? = nil, completion: ((Any) -> Void)? = nil) {
        if isTestLibrary, let operation = command["op"] as? String, operation.hasPrefix("sync_") || operation == "import_v1" {
            fail(CoreError(code: "isolated_test", message: "隔离验证模式不执行云端连接、密钥保存或旧库导入。")); return
        }
        busyCount += 1
        engine.request(command) { [weak self] result in
            guard let self else { return }
            self.busyCount -= 1
            switch result {
            case .success(let value):
                if let message { self.announce(message) }
                self.refreshStatus(); self.reload(force: true); completion?(value)
            case .failure(let error): self.fail(error)
            }
        }
    }
    func patchSettings(_ values: [String: Any], completion: (() -> Void)? = nil) {
        if values["hotkey"] != nil {
            guard values.count == 1, let value = values["hotkey"] as? String else {
                hotkeyError = "请单独保存快捷键，以便先检查占用并安全回滚。"; return
            }
            applyHotkey(value, completion: completion); return
        }
        command(["op": "settings", "values": values]) { [weak self] _ in
            self?.settings.merge(values) { _, new in new }
            self?.onSettingsChanged?(); completion?()
        }
    }
    private func notifyHotkeyRecordingChanged(_ value: Bool) {
        if !isSyntheticSession { onHotkeyRecordingChanged?(value) }
    }
    func beginHotkeyRecording() {
        guard !busy, !isApplyingHotkey, !isRecordingHotkey else { return }
        hotkeyError = nil; hotkeyNotice = nil; hotkeyEditor.begin()
        notifyHotkeyRecordingChanged(true)
    }
    func cancelHotkeyRecording() {
        guard isRecordingHotkey else { return }
        hotkeyEditor.cancel(); notifyHotkeyRecordingChanged(false)
    }
    func updateHotkeyModifiers(_ modifiers: HotkeyModifiers) { hotkeyEditor.updateModifiers(modifiers) }
    @discardableResult func recordHotkey(_ stroke: HotkeyStroke) -> Bool {
        guard isRecordingHotkey else { return false }
        let consumed = hotkeyEditor.receive(stroke)
        if !isRecordingHotkey { notifyHotkeyRecordingChanged(false) }
        return consumed
    }
    /// Carbon may intercept the currently reserved combo before an NSView receives it.
    /// The delegate forwards only registered combos; inactive/unfocused recording ignores it.
    func recordRegisteredHotkey(_ value: String) {
        guard isRecordingHotkey else { return }
        hotkeyEditor.accept(value)
        if !isRecordingHotkey { notifyHotkeyRecordingChanged(false) }
    }
    func restoreDefaultHotkey() { applyHotkey(HotkeySpec.defaultValue) }
    func applyHotkey(_ value: String, completion: (() -> Void)? = nil) {
        guard !isApplyingHotkey, !busy else { return }
        guard !isRecordingHotkey else { hotkeyError = "请先完成或取消按键录入。"; return }
        hotkeyError = nil; hotkeyNotice = nil
        let spec: HotkeySpec
        do { spec = try HotkeySpec.parse(value) }
        catch { hotkeyError = error.localizedDescription; return }
        let finishRegistration = isSyntheticSession ? nil : onHotkeyRegistrationFinished
        if !isSyntheticSession, onRegisterHotkey == nil || finishRegistration == nil {
            hotkeyError = "系统快捷键服务尚未就绪；未保存，也未更改原组合。"; return
        }
        isApplyingHotkey = true; busyCount += 1; hotkeySettingsRevision += 1
        if !isSyntheticSession {
            do { try onRegisterHotkey?(spec.canonical) }
            catch {
                isApplyingHotkey = false; busyCount -= 1; hotkeySettingsRevision += 1
                hotkeyError = error.localizedDescription; return
            }
        }
        // The core persists settings atomically. Keep the old Carbon lease until it commits.
        engine.request(["op": "settings", "values": ["hotkey": spec.canonical]]) { [weak self] result in
            guard let self else { finishRegistration?(false); return }
            switch result {
            case .success:
                finishRegistration?(true)
                self.settings["hotkey"] = spec.canonical
                self.hotkeyEditor.didSave()
                self.hotkeyNotice = self.isTestLibrary ? "已保存到隔离测试库，未注册系统快捷键。" : (self.isDemo ? "仅更新演示设置，未注册系统快捷键。" : "已保存：\(spec.display)")
            case .failure(let error):
                finishRegistration?(false)
                self.hotkeyError = "保存失败，原快捷键与设置保持不变。\n" + error.localizedDescription
            }
            self.hotkeySettingsRevision += 1; self.isApplyingHotkey = false; self.busyCount -= 1
            if case .success = result { self.onSettingsChanged?(); completion?() }
        }
    }
    func enableRecording() { patchSettings(["onboarding_complete": true, "paused": false]) }
    func setPaused(_ value: Bool) { patchSettings(["paused": value]) }
    func setFavorite(_ item: ClipItem, category: String?) { command(["op": "favorite", "id": item.id, "category": category as Any? ?? NSNull()], message: category == nil ? "已移出收藏，记录仍保留在历史中" : "已加入收藏") }
    func saveTags(_ item: ClipItem, input: String) {
        let tags = Array(Set(input.split(whereSeparator: { $0 == "," || $0 == "，" || $0 == "\n" }).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty })).sorted()
        command(["op": "tags", "id": item.id, "tags": tags], message: "标签已保存") { [weak self] _ in self?.editTags = nil }
    }
    func delete(_ item: ClipItem) {
        confirmation = Confirmation(title: "删除这条记录？", message: "删除会同步到已连接的设备。云端操作历史不会被自动清理，不能视为彻底擦除旧正文。", actionTitle: "删除记录") { [weak self] in self?.command(["op": "delete", "id": item.id], message: "删除操作已保存") }
    }
    func clearLocal() {
        confirmation = Confirmation(title: "清空本机历史？", message: "只隐藏这台 Mac 的非收藏历史，不向其他设备发送删除。收藏、附件和云端操作历史仍保留；这不是彻底擦除或释放磁盘空间。", actionTitle: "仅清空本机") { [weak self] in self?.command(["op": "clear_local"], message: "已隐藏本机非收藏历史；未传播删除或清除附件") }
    }
    func requestPreview(_ item: ClipItem) {
        guard item.available else { preview = item; return }
        engine.request(["op": "get", "id": item.id]) { [weak self] result in
            switch result {
            case .success(let value): do { self?.preview = try decodeCore(value, as: ClipItem.self) } catch { self?.fail(error) }
            case .failure(let error): self?.fail(error)
            }
        }
    }
    func pasteSelection(plain: Bool = false, copyOnly: Bool = false) {
        guard !isSyntheticSession else { announce(isDemo ? "演示模式不改写系统剪贴板" : "隔离验证不改写系统剪贴板"); return }
        let selected = activeItems
        // Fetch the complete records; paged results may contain preview-only text.
        fetchComplete(selected.map(\.id)) { [weak self] complete in
            guard let self else { return }
            do {
                let batch = try PasteBatch.make(complete, plain: plain)
                if case .sequence(let queue) = batch {
                    self.sequence = queue; self.sequenceIndex = 0
                    self.announce("顺序队列已准备；每次点击“粘贴下一项”只发送一项")
                } else { _ = self.onPaste?(batch, copyOnly) }
            } catch { self.fail(error) }
        }
    }
    private func fetchComplete(_ ids: [String], completion: @escaping ([ClipItem]) -> Void) {
        var completed: [ClipItem] = []
        func next(_ index: Int) {
            guard index < ids.count else { completion(completed); return }
            engine.request(["op": "get", "id": ids[index]]) { [weak self] result in
                switch result {
                case .success(let value): do { completed.append(try decodeCore(value, as: ClipItem.self)); next(index + 1) } catch { self?.fail(error) }
                case .failure(let error): self?.fail(error)
                }
            }
        }
        next(0)
    }
    func pasteNext() {
        guard !sequencePreparing, sequenceIndex < sequence.count else { return }
        let index = sequenceIndex
        let identifier = sequence[index].id
        sequencePreparing = true
        // Re-read through `get` before every step: Rust verifies attachments and current state.
        engine.request(["op": "get", "id": identifier]) { [weak self] result in
            guard let self else { return }
            self.sequencePreparing = false
            guard self.sequenceIndex == index, self.sequence.indices.contains(index), self.sequence[index].id == identifier else { return }
            switch result {
            case .success(let value):
                do {
                    let item = try decodeCore(value, as: ClipItem.self)
                    guard self.onPaste?(.single(item), false) == true else { return }
                    self.sequenceIndex += 1
                    if self.sequenceIndex == self.sequence.count { self.announce("顺序队列已取完"); self.sequence = []; self.sequenceIndex = 0 }
                } catch { self.fail(error) }
            case .failure(let error): self.fail(error)
            }
        }
    }
    func loadConflicts() {
        conflictGeneration += 1
        let generation = conflictGeneration
        engine.request(["op": "conflicts"]) { [weak self] result in
            switch result {
            case .success(let value):
                // Rich-text fingerprints may cover MiB of source. Prepare once off the UI
                // thread; neither render HTML nor read referenced resources or file paths.
                DispatchQueue.global(qos: .userInitiated).async {
                    let prepared = Result { try decodeCore((value as? [String: Any])?["items"] ?? value, as: [ConflictGroup].self) }
                    DispatchQueue.main.async {
                        guard let self, self.conflictGeneration == generation else { return }
                        switch prepared {
                        case .success(let groups): self.conflicts = groups; self.showConflicts = true
                        case .failure(let error): self.fail(error)
                        }
                    }
                }
            case .failure(let error): self?.fail(error)
            }
        }
    }
    func resolveConflict(entity: String, keep: String) { command(["op": "resolve", "entity": entity, "keep": keep], message: "冲突处理已保存") { [weak self] _ in self?.loadConflicts() } }
    func capture(_ command: [String: Any], completion: (() -> Void)? = nil) {
        guard !paused, !isSyntheticSession else { completion?(); return }
        engine.request(command) { [weak self] result in
            switch result { case .success: self?.reload(force: false, background: true); case .failure(let error): self?.fail(error) }
            completion?()
        }
    }
    func fail(_ error: Error) { errorMessage = error.localizedDescription }
    func announce(_ text: String) {
        notice = text
        DispatchQueue.main.asyncAfter(deadline: .now() + 5) { [weak self] in if self?.notice == text { self?.notice = nil } }
    }
}
