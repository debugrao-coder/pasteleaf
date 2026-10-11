import AppKit
import SwiftUI

struct HotkeyRecorder: NSViewRepresentable {
    @ObservedObject var model: AppModel
    func makeNSView(context: Context) -> HotkeyRecorderButton {
        let button = HotkeyRecorderButton()
        button.model = model
        button.synchronize()
        return button
    }
    func updateNSView(_ nsView: HotkeyRecorderButton, context: Context) {
        nsView.model = model; nsView.synchronize()
    }
    static func dismantleNSView(_ nsView: HotkeyRecorderButton, coordinator: ()) { nsView.stopRecording() }
}

/// No global keyboard monitor. Keys are inspected only after explicit activation while
/// this control is the first responder of the key window, before menu key equivalents.
@MainActor
final class HotkeyRecorderButton: NSButton {
    weak var model: AppModel?
    private var monitor: Any?
    private var observers: [NSObjectProtocol] = []
    private var pressGate = HotkeyPressGate()
    override var acceptsFirstResponder: Bool { true }

    init() {
        super.init(frame: .zero)
        bezelStyle = .rounded; setButtonType(.momentaryPushIn)
        target = self; action = #selector(activateRecording)
        setAccessibilityLabel("呼出剪贴板的快捷键，点按后录入")
        setAccessibilityHelp("按下包含 Control、Option 或 Command 的字母或数字组合。Escape 取消，Tab 移到下一控件。录入后仍需保存。")
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }
    @objc private func activateRecording() {
        guard let model, isEnabled, window?.makeFirstResponder(self) == true else { return }
        if model.isRecordingHotkey { model.cancelHotkeyRecording() }
        else { model.beginHotkeyRecording() }
        synchronize()
    }
    func synchronize() {
        guard let model else { return }
        isEnabled = !model.busy && !model.isApplyingHotkey
        if model.isRecordingHotkey {
            let symbols = model.hotkeyEditor.heldModifiers.display
            title = symbols.isEmpty ? "按下组合键…" : symbols + "…"
            toolTip = "正在录入；Esc 取消，Tab 退出"
            setAccessibilityValue("正在录入 " + symbols)
            installMonitor()
        } else {
            let value = model.hotkeyEditor.draft ?? model.hotkey
            let symbols = (try? HotkeySpec.parse(value).display) ?? value
            title = symbols + (model.hotkeyEditor.draft == nil ? "" : " · 未保存")
            toolTip = "点按录制新的快捷键"
            setAccessibilityValue(symbols + (model.hotkeyEditor.draft == nil ? "，已保存" : "，尚未保存"))
            if pressGate.isEmpty { removeMonitor() }
        }
    }
    override func resignFirstResponder() -> Bool {
        let result = super.resignFirstResponder()
        if result { stopRecording() }
        return result
    }
    override func viewWillMove(toWindow newWindow: NSWindow?) {
        if newWindow !== window {
            stopRecording()
            for observer in observers { NotificationCenter.default.removeObserver(observer) }; observers = []
        }
        super.viewWillMove(toWindow: newWindow)
    }
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        for name in [NSWindow.didResignKeyNotification, NSWindow.willCloseNotification] {
            observers.append(NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.stopRecording() }
            })
        }
    }
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if process(event) == nil { return true }
        return super.performKeyEquivalent(with: event)
    }
    override func keyDown(with event: NSEvent) { if process(event) != nil { super.keyDown(with: event) } }
    override func keyUp(with event: NSEvent) { if process(event) != nil { super.keyUp(with: event) } }
    override func flagsChanged(with event: NSEvent) { if process(event) != nil { super.flagsChanged(with: event) } }

    func stopRecording() {
        model?.cancelHotkeyRecording()
        pressGate.reset(); removeMonitor()
        synchronize()
    }
    private func installMonitor() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { [weak self] event in
            guard let self else { return event }
            return self.process(event)
        }
    }
    private func removeMonitor() { if let monitor { NSEvent.removeMonitor(monitor) }; monitor = nil }
    private func process(_ event: NSEvent) -> NSEvent? {
        guard let model, let window, window.isKeyWindow, window.firstResponder === self,
              event.window == nil || event.window === window, isEnabled else {
            if model?.isRecordingHotkey == true { stopRecording() }
            return event
        }
        if event.type == .keyUp, pressGate.release(event.keyCode) {
            synchronize(); return nil
        }
        if event.type == .keyDown, pressGate.owns(event.keyCode) { return nil }
        guard model.isRecordingHotkey else { return event }
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var modifiers: HotkeyModifiers = []
        if flags.contains(.control) { modifiers.insert(.control) }
        if flags.contains(.option) { modifiers.insert(.option) }
        if flags.contains(.shift) { modifiers.insert(.shift) }
        if flags.contains(.command) { modifiers.insert(.command) }
        if event.type == .flagsChanged { model.updateHotkeyModifiers(modifiers); synchronize(); return nil }
        guard event.type == .keyDown else { return event }
        let consumed = model.recordHotkey(HotkeyStroke(keyCode: event.keyCode, modifiers: modifiers, isRepeat: event.isARepeat, hasUnsupportedModifier: flags.contains(.function)))
        if consumed { pressGate.consume(event.keyCode) }
        synchronize()
        return consumed ? nil : event
    }
    deinit {
        if let monitor { NSEvent.removeMonitor(monitor) }
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
    }
}
