import AppKit
import ApplicationServices
import Carbon
import ServiceManagement
import UniformTypeIdentifiers
import ImageIO

@MainActor
final class ClipboardMonitor {
    private weak var model: AppModel?
    private var timer: Timer?
    private var lastChange = -1
    private var captureInFlight = false
    private let staging: URL
    init(model: AppModel, staging: URL) { self.model = model; self.staging = staging }
    func update() {
        guard let model, !model.isDemo, !model.paused else { stop(); return }
        guard timer == nil else { return }
        // Consent starts observation from now, never imports the current clipboard implicitly.
        lastChange = NSPasteboard.general.changeCount
        timer = Timer.scheduledTimer(withTimeInterval: 0.6, repeats: true) { [weak self] _ in Task { @MainActor in self?.poll() } }
    }
    func stop() { timer?.invalidate(); timer = nil }
    func suppressCurrentChange() { lastChange = NSPasteboard.general.changeCount }
    private func poll() {
        guard let model, !model.isDemo, !model.paused, !captureInFlight else { return }
        let pasteboard = NSPasteboard.general
        guard pasteboard.changeCount != lastChange else { return }
        lastChange = pasteboard.changeCount
        let source = NSWorkspace.shared.frontmostApplication
        let bundle = source?.bundleIdentifier ?? ""
        guard !model.excludedApps.contains(bundle), bundle != Bundle.main.bundleIdentifier else { return }
        var command: [String: Any] = ["op": "capture", "source": source?.localizedName ?? bundle]
        if let urls = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL], !urls.isEmpty {
            guard urls.allSatisfy({ url in
                guard url.isFileURL, let values = try? url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .isUbiquitousItemKey, .ubiquitousItemDownloadingStatusKey]) else { return false }
                return values.isRegularFile == true && values.isSymbolicLink != true && (values.isUbiquitousItem != true || values.ubiquitousItemDownloadingStatus == .current)
            }) else { model.announce("跳过目录、符号链接或尚未下载的文件；只捕获完整普通文件。"); return }
            command["kind"] = "files"; command["text"] = urls.map(\.lastPathComponent).joined(separator: "\n"); command["paths"] = urls.map(\.path)
        } else if let imageData = pasteboard.data(forType: .png) ?? pasteboard.data(forType: .tiff) {
            do { try SafeImage.validate(imageData) } catch { model.fail(error); return }
            captureInFlight = true
            let sourceName = command["source"] as? String ?? ""
            let staging = self.staging
            DispatchQueue.global(qos: .utility).async { [weak self, weak model] in
                do {
                    let png = try SafeImage.pngData(imageData)
                    try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
                    let path = staging.appendingPathComponent(UUID().uuidString).appendingPathExtension("png")
                    try png.write(to: path, options: [.atomic])
                    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: path.path)
                    DispatchQueue.main.async {
                        guard let model else { try? FileManager.default.removeItem(at: path); self?.captureInFlight = false; return }
                        model.capture(["op": "capture", "kind": "image", "text": "", "image_path": path.path, "source": sourceName]) {
                            self?.captureInFlight = false
                            try? FileManager.default.removeItem(at: path)
                        }
                    }
                } catch { DispatchQueue.main.async { self?.captureInFlight = false; model?.fail(error) } }
            }
            return
        } else {
            let text = pasteboard.string(forType: .string) ?? ""
            let html = pasteboard.string(forType: .html)
            let rtf = pasteboard.data(forType: .rtf)
            guard !text.isEmpty || html != nil || rtf != nil else { return }
            command["kind"] = (html != nil || rtf != nil) ? "rich_text" : "text"
            command["text"] = text
            if let html { command["html"] = html }
            if let rtf { command["rtf"] = rtf.base64EncodedString() }
        }
        captureInFlight = true
        model.capture(command) { [weak self] in
            self?.captureInFlight = false
        }
    }
}

@MainActor
final class GlobalHotkey {
    private var references: [UInt32: EventHotKeyRef] = [:]
    private var recordedUntilRelease: Set<UInt32> = []
    private var handler: EventHandlerRef?
    private lazy var plan = HotkeyRegistrationPlan(reserve: { [weak self] registration in
        guard let self else { throw CoreError(code: "hotkey", message: "快捷键服务已关闭。") }
        try self.reserve(registration)
    }, release: { [weak self] registration in
        if let reference = self?.references.removeValue(forKey: registration.id) { UnregisterEventHotKey(reference) }
        self?.recordedUntilRelease.remove(registration.id)
    })
    var action: (() -> Void)?
    var suspended = false
    var onRecordedHotkey: ((String) -> Void)?

    func register(_ value: String) throws { try plan.register(HotkeySpec.parse(value)) }
    func prepare(_ value: String) throws { try plan.prepare(HotkeySpec.parse(value)) }
    func finishUpdate(commit: Bool) { plan.finish(commit: commit) }
    func unregister() {
        plan.unregisterAll()
        recordedUntilRelease = []
        if let handler { RemoveEventHandler(handler) }; handler = nil
    }
    private func receive(_ id: UInt32, released: Bool) {
        if released { recordedUntilRelease.remove(id); return }
        if recordedUntilRelease.contains(id) { return }
        guard let spec = plan.spec(for: id) else { return }
        if suspended { recordedUntilRelease.insert(id); onRecordedHotkey?(spec.canonical); return }
        // A reserved proposal cannot invoke the panel before persistence succeeds.
        guard plan.active?.id == id else { return }
        action?()
    }
    private func reserve(_ registration: HotkeyRegistrationPlan.Registration) throws {
        let spec = registration.spec
        guard let code = HotkeySpec.keyCodes[spec.key] else { throw CoreError(code: "hotkey", message: "不支持这个按键。") }
        var flags: UInt32 = 0
        if spec.command { flags |= UInt32(cmdKey) }; if spec.option { flags |= UInt32(optionKey) }
        if spec.control { flags |= UInt32(controlKey) }; if spec.shift { flags |= UInt32(shiftKey) }
        if handler == nil {
            let events = [kEventHotKeyPressed, kEventHotKeyReleased].map { EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32($0)) }
            let callback: EventHandlerUPP = { _, event, context in
                guard let context, let event else { return OSStatus(eventNotHandledErr) }
                var identifier = EventHotKeyID()
                let status = GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID), nil, MemoryLayout<EventHotKeyID>.size, nil, &identifier)
                guard status == noErr, identifier.signature == 0x504C4546 else { return OSStatus(eventNotHandledErr) }
                let instance = Unmanaged<GlobalHotkey>.fromOpaque(context).takeUnretainedValue()
                let id = identifier.id
                let released = GetEventKind(event) == UInt32(kEventHotKeyReleased)
                DispatchQueue.main.async { instance.receive(id, released: released) }
                return noErr
            }
            let result = events.withUnsafeBufferPointer { buffer in
                InstallEventHandler(GetApplicationEventTarget(), callback, buffer.count, buffer.baseAddress, Unmanaged.passUnretained(self).toOpaque(), &handler)
            }
            guard result == noErr else { throw CoreError(code: "hotkey", message: "无法安装全局快捷键事件处理器。") }
        }
        var proposed: EventHotKeyRef?
        let result = RegisterEventHotKey(code, flags, EventHotKeyID(signature: 0x504C4546, id: registration.id), GetApplicationEventTarget(), 0, &proposed)
        guard result == noErr, let proposed else {
            if let proposed { UnregisterEventHotKey(proposed) }
            throw CoreError(code: "hotkey_conflict", message: "\(spec.display) 已被占用或无法注册。原快捷键仍保留，未保存或抢占其他应用的组合。")
        }
        references[registration.id] = proposed
    }
}

@MainActor
final class PasteController {
    private weak var model: AppModel?
    private weak var monitor: ClipboardMonitor?
    private var target: NSRunningApplication?
    private var targetWindow: AXUIElement?
    var hide: (() -> Void)?
    init(model: AppModel, monitor: ClipboardMonitor) { self.model = model; self.monitor = monitor }
    func rememberTarget() {
        guard let front = NSWorkspace.shared.frontmostApplication, front.processIdentifier != ProcessInfo.processInfo.processIdentifier else { return }
        target = front; targetWindow = nil
        guard AXIsProcessTrusted() else { return }
        var value: CFTypeRef?
        if AXUIElementCopyAttributeValue(AXUIElementCreateApplication(front.processIdentifier), kAXFocusedWindowAttribute as CFString, &value) == .success, let value, CFGetTypeID(value) == AXUIElementGetTypeID() {
            targetWindow = (value as! AXUIElement)
        }
    }
    func requestPermission() {
        let options = [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(options)
    }
    func paste(_ batch: PasteBatch, copyOnly: Bool) -> Bool {
        guard let model, !model.isSyntheticSession else { return false }
        do {
            try write(batch)
            monitor?.suppressCurrentChange()
        } catch { model.fail(error); return false }
        guard !copyOnly else { model.announce("已复制到剪贴板"); return true }
        guard AXIsProcessTrusted(), let target, !target.isTerminated, let originalWindow = targetWindow else {
            model.announce("已复制；请切回目标应用手动粘贴。自动粘贴需辅助功能权限。")
            return true
        }
        hide?()
        guard target.activate(options: []) else { model.announce("已复制；目标应用未能激活，请手动粘贴。"); return true }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.16) { [weak self] in
            guard let self, NSWorkspace.shared.frontmostApplication?.processIdentifier == target.processIdentifier else {
                self?.model?.announce("已复制，但目标窗口未恢复；请手动粘贴。")
                return
            }
            guard AXUIElementPerformAction(originalWindow, kAXRaiseAction as CFString) == .success else {
                self.model?.announce("已复制，但原窗口无法恢复；未发送快捷键，请手动粘贴。")
                return
            }
            var focused: CFTypeRef?
            let focusedResult = AXUIElementCopyAttributeValue(AXUIElementCreateApplication(target.processIdentifier), kAXFocusedWindowAttribute as CFString, &focused)
            guard focusedResult == .success, let focused, CFGetTypeID(focused) == AXUIElementGetTypeID(), CFEqual(focused, originalWindow) else {
                self.model?.announce("已复制，但当前不是原窗口；未发送快捷键，请手动粘贴。")
                return
            }
            guard let source = CGEventSource(stateID: .combinedSessionState), let down = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: true), let up = CGEvent(keyboardEventSource: source, virtualKey: 9, keyDown: false) else {
                self.model?.announce("已复制；无法生成粘贴快捷键，请手动粘贴。")
                return
            }
            down.flags = .maskCommand; up.flags = .maskCommand
            down.post(tap: .cghidEventTap); up.post(tap: .cghidEventTap)
            self.model?.announce("已向原窗口发送粘贴快捷键")
        }
        return true
    }
    private func write(_ batch: PasteBatch) throws {
        let pasteboard = NSPasteboard.general
        switch batch {
        case .sequence: throw CoreError(code: "sequence", message: "顺序队列需要逐项触发。")
        case .text(let text): pasteboard.clearContents(); guard pasteboard.setString(text, forType: .string) else { throw writeError }
        case .files(let files): try writeFiles(files, to: pasteboard)
        case .single(let item):
            guard item.available else { throw CoreError(code: "not_available", message: "附件尚未可用。") }
            if item.kind == "files" { try writeFiles(item.files, to: pasteboard); return }
            let object = NSPasteboardItem()
            if !item.text.isEmpty { object.setString(item.text, forType: .string) }
            if let html = item.html { object.setString(html, forType: .html) }
            if let rtf = item.rtf {
                guard let bytes = Data(base64Encoded: rtf) else { throw CoreError(code: "rtf_corrupt", message: "富文本内容未通过校验，未改写剪贴板。") }
                object.setData(bytes, forType: .rtf)
            }
            if item.kind == "image" {
                guard let path = item.imagePath, let data = try? Data(contentsOf: URL(fileURLWithPath: path), options: [.mappedIfSafe]) else { throw CoreError(code: "image_missing", message: "图片文件不可用，请等待同步或检查本地资料库。") }
                let type = try SafeImage.pasteboardType(data)
                // Advertise the actual verified UTI: remote JPEG must never be labelled PNG.
                // This preserves bytes without a full-size raster allocation on the UI thread.
                object.setData(data, forType: NSPasteboard.PasteboardType(type))
            }
            pasteboard.clearContents(); guard pasteboard.writeObjects([object]) else { throw writeError }
        }
    }
    private func writeFiles(_ files: [ClipFile], to pasteboard: NSPasteboard) throws {
        guard !files.isEmpty else { throw CoreError(code: "empty_files", message: "记录不包含可粘贴的文件。") }
        var urls: [NSURL] = []
        for file in files {
            let url = URL(fileURLWithPath: file.path)
            guard let resource = try? url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey]), resource.isRegularFile == true, resource.isSymbolicLink != true else { throw CoreError(code: "file_missing", message: "有附件不在本机或尚未下载完成，未改写剪贴板。") }
            urls.append(url as NSURL)
        }
        pasteboard.clearContents(); guard pasteboard.writeObjects(urls) else { throw writeError }
    }
    private var writeError: Error { CoreError(code: "clipboard_write", message: "无法写入系统剪贴板，请重试。") }
}

@MainActor
enum NativeFolders {
    static func pick(title: String, completion: @escaping (URL?) -> Void) {
        let panel = NSOpenPanel()
        panel.title = title; panel.prompt = "选择目录"
        panel.canChooseFiles = false; panel.canChooseDirectories = true; panel.canCreateDirectories = false; panel.allowsMultipleSelection = false
        let cloud = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Mobile Documents/com~apple~CloudDocs")
        if FileManager.default.fileExists(atPath: cloud.path) { panel.directoryURL = cloud }
        panel.begin { response in completion(response == .OK ? panel.url : nil) }
    }
    static func pickApplication(completion: @escaping (String?) -> Void) {
        let panel = NSOpenPanel(); panel.title = "选择不记录的应用"; panel.prompt = "排除此应用"
        panel.allowedContentTypes = [.applicationBundle]; panel.canChooseDirectories = false; panel.allowsMultipleSelection = false
        panel.directoryURL = URL(fileURLWithPath: "/Applications")
        panel.begin { response in completion(response == .OK ? panel.url.flatMap { Bundle(url: $0)?.bundleIdentifier } : nil) }
    }
    static func setLogin(enabled: Bool) throws {
        if enabled { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
    }
}
