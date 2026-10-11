import AppKit
import Combine
import SwiftUI

/// Settings is a peer window, never a sheet or child of the bottom shelf.
/// Only this controller can change its geometry; opening it cannot resize the shelf.
@MainActor
final class SettingsWindowController: NSWindowController, NSWindowDelegate {
    private let model: AppModel
    private var appearanceObserver: AnyCancellable?
    private var screenObserver: NSObjectProtocol?
    private var previousFrame: NSRect?
    var onClose: (() -> Void)?

    init(model: AppModel) {
        self.model = model
        super.init(window: nil)
        appearanceObserver = model.$settings.receive(on: RunLoop.main).sink { [weak self] settings in
            self?.applyAppearance(settings["appearance"] as? String ?? "system")
        }
        screenObserver = NotificationCenter.default.addObserver(forName: NSApplication.didChangeScreenParametersNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.fitToCurrentScreen() }
        }
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    func present(in visibleFrame: NSRect) {
        if window == nil {
            let window = NSWindow(contentRect: NSRect(origin: .zero, size: SettingsWindowLayout.preferredContentSize),
                                  styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
            window.title = model.isSyntheticSession ? "PasteLeaf 设置 · 隔离验证" : "PasteLeaf 设置"
            window.setAccessibilityLabel(window.title)
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.level = .normal
            window.tabbingMode = .disallowed
            window.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary]
            window.contentMinSize = NSSize(width: min(540, visibleFrame.width), height: min(420, max(0, visibleFrame.height - 40)))
            window.contentView = nil
            self.window = window
        }
        guard let window else { return }
        if window.contentView == nil {
            let host = NSHostingView(rootView: SettingsView(model: model, close: { [weak self] in self?.closeSettings() }))
            // SwiftUI's ideal form size must not move or resize either native window.
            host.sizingOptions = []
            window.contentView = host
        }
        if !window.isVisible {
            let preferred = window.frameRect(forContentRect: NSRect(origin: .zero, size: SettingsWindowLayout.preferredContentSize)).size
            window.setFrame(SettingsWindowLayout.frame(in: visibleFrame, preferredSize: preferred, previous: window.isMiniaturized ? window.frame : previousFrame), display: false)
        }
        fitToCurrentScreen()
        applyAppearance(model.appearance)
        if window.isMiniaturized { window.deminiaturize(nil) }
        NSApp.unhide(nil)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    func closeSettings() {
        guard let window, window.contentView != nil else { return }
        guard !model.isApplyingHotkey else { model.showSettings = true; model.announce("快捷键正在保存，请稍候。"); return }
        window.performClose(nil)
    }
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard !model.isApplyingHotkey else { model.announce("快捷键正在保存，请稍候。"); return false }
        return sender.attachedSheet == nil
    }
    func windowDidResignKey(_ notification: Notification) { model.cancelHotkeyRecording() }
    func windowWillClose(_ notification: Notification) {
        model.cancelHotkeyRecording()
        previousFrame = window?.frame
        // Tear down the form, including transient sync-password drafts and event monitors.
        window?.contentView = nil
        onClose?()
    }
    private func applyAppearance(_ value: String) {
        window?.appearance = value == "dark" ? NSAppearance(named: .darkAqua) : (value == "light" ? NSAppearance(named: .aqua) : nil)
    }
    private func fitToCurrentScreen() {
        guard let window, window.contentView != nil,
              let screen = window.screen ?? NSScreen.screens.first(where: { $0.visibleFrame.intersects(window.frame) }) ?? NSScreen.main else { return }
        let visible = screen.visibleFrame
        window.contentMinSize = NSSize(width: min(540, visible.width), height: min(420, max(0, visible.height - 40)))
        let frame = SettingsWindowLayout.frame(in: visible, preferredSize: window.frame.size, previous: window.frame)
        if frame != window.frame { window.setFrame(frame, display: true) }
    }
    deinit { if let screenObserver { NotificationCenter.default.removeObserver(screenObserver) } }
}
