import AppKit
import Combine
import SwiftUI

@main
struct PasteLeafApplication {
    @MainActor static func main() {
        let app = NSApplication.shared
        let delegate = ApplicationDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.accessory)
        withExtendedLifetime(delegate) { app.run() }
    }
}

final class LeafPanel: NSPanel {
    var handleKey: ((NSEvent) -> Bool)?
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }
    override func sendEvent(_ event: NSEvent) {
        if event.type == .keyDown, handleKey?(event) == true { return }
        super.sendEvent(event)
    }
}

@MainActor
final class ApplicationDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var model: AppModel!
    private var panel: LeafPanel!
    private var statusItem: NSStatusItem!
    private var monitor: ClipboardMonitor?
    private var pasteController: PasteController?
    private let hotkey = GlobalHotkey()
    private var failedHotkey: String?
    private var settingsController: SettingsWindowController?
    private var settingsObserver: AnyCancellable?
    private var workspaceObservers: [NSObjectProtocol] = []
    private var appMenu: NSMenu!
    private var isDemo = false
    private var isTestLibrary = false
    private var isolatedRootPath: String?
    private var lifecycle = LifecyclePolicy(arguments: [], environment: [:], bundleInfo: [:])

    func applicationDidFinishLaunching(_ notification: Notification) {
        let arguments = ProcessInfo.processInfo.arguments
        lifecycle = LifecyclePolicy(arguments: arguments, environment: ProcessInfo.processInfo.environment, bundleInfo: Bundle.main.infoDictionary ?? [:])
        isTestLibrary = lifecycle.mode == .isolatedTest
        isDemo = lifecycle.mode == .demo
        let engine: ClipboardEngine
        if isDemo { engine = DemoEngine() }
        else if isTestLibrary {
            let index = arguments.firstIndex(of: "--test-library")
            let argumentRoot = index.flatMap { arguments.indices.contains($0 + 1) ? arguments[$0 + 1] : nil }
            let bundleRoot = Bundle.main.object(forInfoDictionaryKey: "PLTestLibrary") as? Bool == true ? Bundle.main.object(forInfoDictionaryKey: "PLTestLibraryRoot") as? String : nil
            guard let path = argumentRoot ?? ProcessInfo.processInfo.environment["PASTELEAF_DATA_ROOT"] ?? bundleRoot, path.hasPrefix("/"), !path.hasPrefix("--"), path != "/" else {
                fputs("Isolated test mode requires --test-library /absolute/synthetic/root or PASTELEAF_DATA_ROOT. No default data directory was opened.\n", stderr)
                NSApp.terminate(nil); return
            }
            setenv("PASTELEAF_DISABLE_SYNC_WORKER", "1", 1)
            setenv("PASTELEAF_DISABLE_KEYCHAIN", "1", 1)
            isolatedRootPath = path
            engine = RustEngine(root: URL(fileURLWithPath: path, isDirectory: true))
        } else {
            let defaultRoot = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("PasteLeaf v2", isDirectory: true)
            let root = ProcessInfo.processInfo.environment["PASTELEAF_DATA_ROOT"].map { URL(fileURLWithPath: $0, isDirectory: true) } ?? defaultRoot
            engine = RustEngine(root: root)
        }
        model = AppModel(engine: engine, systemIntegrationAllowed: !isTestLibrary)
        if !isDemo && !isTestLibrary {
            let staging = FileManager.default.temporaryDirectory.appendingPathComponent("io.pasteleaf.capture.\(ProcessInfo.processInfo.processIdentifier)")
            let clipboard = ClipboardMonitor(model: model, staging: staging)
            monitor = clipboard
            let paste = PasteController(model: model, monitor: clipboard)
            pasteController = paste
            paste.hide = { [weak self] in self?.hide() }
            model.onPaste = { [weak paste] batch, copyOnly in paste?.paste(batch, copyOnly: copyOnly) ?? false }
            model.onPermissionRequest = { [weak paste] in paste?.requestPermission() }
            model.onSettingsChanged = { [weak self] in self?.applySettings() }
            model.onRegisterHotkey = { [weak self] value in try self?.hotkey.prepare(value) }
            model.onHotkeyRegistrationFinished = { [weak self] commit in
                self?.hotkey.finishUpdate(commit: commit)
                if commit { self?.failedHotkey = nil }
            }
            model.onHotkeyRecordingChanged = { [weak self] recording in self?.hotkey.suspended = recording }
            hotkey.onRecordedHotkey = { [weak self] value in self?.model.recordRegisteredHotkey(value) }
            model.onLoginChange = { [weak self] enabled in
                do { try NativeFolders.setLogin(enabled: enabled); self?.model.patchSettings(["launch_at_login": enabled]) }
                catch { self?.model.fail(error) }
            }
            workspaceObservers.append(NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
                Task { @MainActor in self?.model.command(["op": "sync_now"]) }
            })
        }
        model.onHide = { [weak self] in self?.hide() }
        model.onShowPanel = { [weak self] in self?.show() }
        hotkey.action = { [weak self] in self?.toggle() }
        configureMenu()
        configurePanel()
        configureSettingsPresentation()
        model.start()
        show()
        if let index = arguments.firstIndex(of: "--appearance"), arguments.indices.contains(index + 1), isDemo {
            model.patchSettings(["appearance": arguments[index + 1]])
        }
        if let index = arguments.firstIndex(of: "--demo-state"), arguments.indices.contains(index + 1), isDemo {
            let state = arguments[index + 1]
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
                guard let self else { return }
                if state == "empty" { self.model.command(["op": "clear_local", "all": true]) }
                else if state == "no-results" { self.model.searchChanged("没有匹配的合成关键词") }
                else if state == "settings" { self.model.showSettings = true }
                else if state == "sync" { self.model.settingsTab = "sync"; self.model.showSettings = true }
                else if state == "error" { self.model.errorMessage = "合成错误：磁盘保护上限已达到，新增记录已暂停。旧记录没有删除。" }
                else if state == "preview", let item = self.model.items.first { self.model.requestPreview(item) }
            }
        }
    }
    private func activeVisibleFrame() -> NSRect {
        let mouse = NSEvent.mouseLocation
        let screen = NSScreen.screens.first(where: { $0.frame.contains(mouse) }) ?? NSScreen.main
        return screen?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1280, height: 800)
    }
    private func configurePanel() {
        let safe = activeVisibleFrame()
        let frame = PanelLayout.frame(in: safe)
        panel = LeafPanel(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.title = isDemo ? "PasteLeaf — 安全演示" : (isTestLibrary ? "PasteLeaf — 隔离验证" : "PasteLeaf")
        panel.level = .floating; panel.isOpaque = false; panel.backgroundColor = .clear; panel.hasShadow = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.isMovableByWindowBackground = false; panel.hidesOnDeactivate = false
        panel.delegate = self; panel.isReleasedWhenClosed = false
        panel.contentView = NSHostingView(rootView: MainView(model: model))
        panel.handleKey = { [weak self] event in self?.handle(event) ?? false }
        panel.setAccessibilityLabel(isDemo ? "PasteLeaf 演示剪贴板面板" : "PasteLeaf 剪贴板面板")
    }
    private func configureMenu() {
        let main = NSMenu(); let root = NSMenuItem(); main.addItem(root)
        let application = NSMenu(); root.submenu = application
        let mainShow = application.addItem(withTitle: "显示 PasteLeaf", action: #selector(showFromMenu), keyEquivalent: "")
        mainShow.target = self
        let mainSettings = application.addItem(withTitle: "设置…", action: #selector(settingsFromMenu), keyEquivalent: ",")
        mainSettings.target = self
        application.addItem(.separator())
        let mainQuit = application.addItem(withTitle: "退出 PasteLeaf", action: #selector(quit), keyEquivalent: "q")
        mainQuit.target = self
        let editRoot = NSMenuItem(title: "编辑", action: nil, keyEquivalent: ""); main.addItem(editRoot)
        let edit = NSMenu(title: "编辑"); editRoot.submenu = edit
        edit.addItem(withTitle: "撤销", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "剪切", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "复制", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "粘贴", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "全选", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        NSApp.mainMenu = main
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        statusItem.button?.image = NSImage(systemSymbolName: "leaf", accessibilityDescription: "PasteLeaf")
        statusItem.button?.image?.isTemplate = true
        appMenu = NSMenu()
        let show = appMenu.addItem(withTitle: isDemo ? "显示 PasteLeaf 演示" : "显示 PasteLeaf", action: #selector(showFromMenu), keyEquivalent: "")
        show.target = self
        let pause = appMenu.addItem(withTitle: "暂停 / 恢复记录", action: #selector(toggleRecording), keyEquivalent: ""); pause.target = self; pause.isEnabled = !isDemo && !isTestLibrary
        let settings = appMenu.addItem(withTitle: "设置…", action: #selector(settingsFromMenu), keyEquivalent: ""); settings.target = self
        appMenu.addItem(.separator())
        let quitItem = appMenu.addItem(withTitle: "退出", action: #selector(quit), keyEquivalent: ""); quitItem.target = self
        statusItem.menu = appMenu
    }
    private func configureSettingsPresentation() {
        let controller = SettingsWindowController(model: model)
        controller.onClose = { [weak self] in
            self?.model.showSettings = false
            self?.recordWindowGeometry("settings-closed")
        }
        settingsController = controller
        // Do not deduplicate: choosing Settings again must focus the existing window.
        settingsObserver = model.$showSettings.dropFirst().receive(on: RunLoop.main).sink { [weak self] requested in
            guard let self else { return }
            if requested {
                guard self.panel.attachedSheet == nil else {
                    self.model.showSettings = false
                    self.panel.attachedSheet?.makeKeyAndOrderFront(nil)
                    return
                }
                self.recordWindowGeometry("before-settings")
                self.hide()
                controller.present(in: self.activeVisibleFrame())
                self.recordWindowGeometry("settings-opened")
            } else { controller.closeSettings() }
        }
    }
    private func applySettings() {
        monitor?.update()
        guard model.onboardingComplete, !model.isApplyingHotkey, !isDemo, !isTestLibrary else { return }
        if failedHotkey != model.hotkey {
            do { try hotkey.register(model.hotkey); failedHotkey = nil }
            catch { failedHotkey = model.hotkey; model.fail(error) }
        }
    }
    @objc private func showFromMenu() { show() }
    @objc private func settingsFromMenu() { model.showSettings = true }
    @objc private func toggleRecording() { if model.onboardingComplete { model.setPaused(!model.paused) } else { show() } }
    @objc private func quit() { NSApp.terminate(nil) }
    private func toggle() { panel.isKeyWindow && panel.isVisible ? hide() : show() }
    private func show() {
        // Editors and confirmations have exactly one owning window. A hotkey
        // returns to the shelf only after Settings can safely finish closing.
        if model.showSettings {
            settingsController?.closeSettings()
            guard !model.showSettings else { return }
        }
        pasteController?.rememberTarget()
        let safe = activeVisibleFrame()
        panel.setFrame(PanelLayout.frame(in: safe), display: false)
        model.isPanelVisible = true
        NSApp.unhide(nil)
        panel.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        panel.makeFirstResponder(nil)
        recordWindowGeometry("shelf-shown")
    }
    private func hide() {
        guard panel.attachedSheet == nil else { return }
        model.isPanelVisible = false
        panel.orderOut(nil)
    }
    func windowDidResignKey(_ notification: Notification) {
        guard let panel, let model, let window = notification.object as? NSWindow, window === panel else { return }
        let hasPresentedContent = model.preview != nil || model.editTags != nil || model.showCategoryEditor || model.showConflicts || model.confirmation != nil
        // Native folder pickers and sheets temporarily resign key; do not dismiss beneath them.
        if lifecycle.shouldHideOnResignKey(hasAttachedSheet: panel.attachedSheet != nil, hasPresentedContent: hasPresentedContent, hasModalWindow: NSApp.modalWindow != nil) { hide() }
    }
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        if model?.showSettings == true, settingsController?.window?.contentView != nil {
            settingsController?.present(in: activeVisibleFrame())
            return false
        }
        // AppKit's default reopen path does not count NSPanel. Our own panel is authoritative,
        // including when another sheet/window made hasVisibleWindows true.
        switch lifecycle.reopenAction(panelExists: panel != nil, panelIsVisible: panel?.isVisible ?? false, panelIsMiniaturized: panel?.isMiniaturized ?? false) {
        case .awaitInitialLaunch:
            break // applicationDidFinishLaunching will create and show exactly one panel.
        case .showPanel:
            if panel.isMiniaturized { panel.deminiaturize(nil) }
            show()
        case .focusExistingPanel:
            // Preserve the active editor, selection and sheet when merely bringing it forward.
            model.isPanelVisible = true
            sender.unhide(nil)
            panel.makeKeyAndOrderFront(nil)
            sender.activate(ignoringOtherApps: true)
            panel.attachedSheet?.makeKeyAndOrderFront(nil)
        }
        return false // Already handled; never start AppKit's untitled-document fallback.
    }
    private func handle(_ event: NSEvent) -> Bool {
        guard panel.attachedSheet == nil, model.preview == nil, model.editTags == nil, !model.showCategoryEditor, !model.showConflicts, model.confirmation == nil else { return false }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if let text = panel.firstResponder as? NSTextView {
            if text.hasMarkedText() { return false }
            // Native editors own every editing shortcut, including Return during search.
            return false
        }
        if flags.contains(.command), event.charactersIgnoringModifiers?.lowercased() == "f" { NotificationCenter.default.post(name: .focusPasteLeafSearch, object: nil); return true }
        if flags.contains(.command), event.charactersIgnoringModifiers?.lowercased() == "a" { model.selectAll(); return true }
        if flags.contains(.command), event.charactersIgnoringModifiers?.lowercased() == "c" { model.pasteSelection(copyOnly: true); return true }
        if flags.contains(.command), event.charactersIgnoringModifiers == "," { model.showSettings = true; return true }
        switch event.keyCode {
        case 123: model.moveSelection(-1, extend: flags.contains(.shift)); return true
        case 124: model.moveSelection(1, extend: flags.contains(.shift)); return true
        case 49: if let item = model.focusedItem { model.requestPreview(item) }; return true
        case 36, 76: if !model.sequence.isEmpty { model.pasteNext() } else { model.pasteSelection(plain: flags.contains(.shift)) }; return true
        case 53: if !model.sequence.isEmpty { model.sequence = []; model.sequenceIndex = 0 } else if !model.query.isEmpty { model.searchChanged("") } else { hide() }; return true
        default: return false
        }
    }
    func applicationWillTerminate(_ notification: Notification) {
        model?.cancelHotkeyRecording()
        settingsObserver?.cancel()
        model?.stop(); monitor?.stop(); hotkey.unregister()
        for observer in workspaceObservers { NSWorkspace.shared.notificationCenter.removeObserver(observer) }
    }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard model?.isApplyingHotkey != true else {
            model.announce("快捷键正在保存，请稍候再退出。")
            return .terminateCancel
        }
        return .terminateNow
    }
    /// Opt-in synthetic UI evidence only. Never emits titles, clipboard bodies or settings.
    private func recordWindowGeometry(_ phase: String) {
        guard isTestLibrary, Bundle.main.object(forInfoDictionaryKey: "PLWindowDiagnostics") as? Bool == true,
              let path = isolatedRootPath, let panel else { return }
        func rect(_ value: NSRect) -> [String: CGFloat] { ["x": value.minX, "y": value.minY, "width": value.width, "height": value.height] }
        var snapshot: [String: Any] = ["phase": phase, "shelf": rect(panel.frame), "shelf_visible": panel.isVisible,
                                       "attached_sheet": panel.attachedSheet != nil, "synthetic_only": true]
        if let window = settingsController?.window { snapshot["settings"] = rect(window.frame); snapshot["settings_parent"] = window.parent != nil }
        guard var bytes = try? JSONSerialization.data(withJSONObject: snapshot, options: [.sortedKeys]) else { return }
        bytes.append(0x0A)
        let url = URL(fileURLWithPath: path).appendingPathComponent("window-geometry.jsonl")
        if !FileManager.default.fileExists(atPath: url.path) { FileManager.default.createFile(atPath: url.path, contents: nil, attributes: [.posixPermissions: 0o600]) }
        guard let handle = try? FileHandle(forWritingTo: url) else { return }
        defer { try? handle.close() }
        _ = try? handle.seekToEnd(); try? handle.write(contentsOf: bytes)
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
}
