import AppKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

final class ManualEngine: ClipboardEngine {
    let isDemo: Bool
    init(isDemo: Bool = false) { self.isDemo = isDemo }
    var requests: [([String: Any], (Result<Any, Error>) -> Void)] = []
    func request(_ command: [String: Any], completion: @escaping (Result<Any, Error>) -> Void) { requests.append((command, completion)) }
    func reply(_ index: Int, _ value: Any) { requests[index].1(.success(value)) }
    func fail(_ index: Int, _ error: Error) { requests[index].1(.failure(error)) }
}

@main
struct MacTests {
    private static var count = 0
    private static func check(_ condition: @autoclosure () throws -> Bool, _ name: String) throws {
        guard try condition() else { throw CoreError(code: "test", message: "FAIL: " + name) }
        count += 1
    }
    private static func request(_ engine: ClipboardEngine, _ command: [String: Any]) async throws -> Any {
        try await withCheckedThrowingContinuation { continuation in engine.request(command) { continuation.resume(with: $0) } }
    }
    private static func itemJSON(id: String, text: String) -> [String: Any] { ["id": id, "kind": "text", "text": text, "copied_at": 1, "available": true] }
    private static func nativeAppearanceTests() throws {
        let mac14 = NativeAppearancePolicy(macOSMajorVersion: 14)
        let mac15 = NativeAppearancePolicy(macOSMajorVersion: 15)
        let mac26 = NativeAppearancePolicy(macOSMajorVersion: 26)
        let mac27 = NativeAppearancePolicy(macOSMajorVersion: 27)
        try check(mac14.surfaceMode == .vibrancy && mac15.surfaceMode == .vibrancy, "macOS 14 and 15 use native vibrancy instead of unavailable glass APIs")
        try check(mac26.surfaceMode == .liquidGlass && mac27.surfaceMode == .liquidGlass, "macOS 26 and 27 select Liquid Glass")
        try check(mac26.allowsInteractiveGlass && mac27.allowsAnimations, "default native effects remain interactive")
        try check(!mac14.allowsInteractiveGlass && !mac15.allowsInteractiveGlass, "fallback systems never request interactive glass")
        try check(NativeAppearancePolicy(macOSMajorVersion: 0).surfaceMode == .opaque, "unknown or unsupported version fails to an opaque surface")
        try check(NativeAppearancePolicy(macOSMajorVersion: -1).surfaceMode == .opaque, "invalid negative version never enables a translucent surface")
        try check(NativeAppearancePolicy(macOSMajorVersion: 28).surfaceMode == .liquidGlass, "future supported systems retain glass availability without an upper bound")

        let appOpaque = NativeAppearancePolicy(macOSMajorVersion: 27, appReduceTransparency: true)
        let systemOpaque = NativeAppearancePolicy(macOSMajorVersion: 27, systemReduceTransparency: true)
        try check(appOpaque.surfaceMode == .opaque && appOpaque.reduceTransparency, "app reduce transparency disables glass")
        try check(systemOpaque.surfaceMode == .opaque && systemOpaque.reduceTransparency, "system reduce transparency cannot be overridden by app preference")
        try check(!appOpaque.allowsInteractiveGlass && !systemOpaque.allowsInteractiveGlass, "opaque surfaces cannot request interactive glass")
        try check(appOpaque.allowsAnimations && systemOpaque.allowsAnimations, "transparency and motion preferences stay independent")
        let contrast = NativeAppearancePolicy(macOSMajorVersion: 27, systemIncreaseContrast: true)
        try check(contrast.surfaceMode == .opaque && contrast.reduceTransparency, "increased contrast disables transparent effects even without reduce transparency")
        try check(contrast.increaseContrast && contrast.requiresButtonBorders, "increased contrast demands explicit control boundaries")

        let appMotion = NativeAppearancePolicy(macOSMajorVersion: 27, appReduceMotion: true)
        let systemMotion = NativeAppearancePolicy(macOSMajorVersion: 26, systemReduceMotion: true)
        try check(appMotion.reduceMotion && !appMotion.allowsAnimations, "app reduce motion disables ornamental transitions")
        try check(systemMotion.reduceMotion && !systemMotion.allowsAnimations, "system reduce motion cannot be overridden by app preference")
        try check(!appMotion.allowsInteractiveGlass && !systemMotion.allowsInteractiveGlass, "reduce motion suppresses reactive glass animation")
        try check(appMotion.surfaceMode == .liquidGlass && systemMotion.surfaceMode == .liquidGlass, "reduce motion preserves static glass unless transparency is disabled")
        let borders27 = NativeAppearancePolicy(macOSMajorVersion: 27, systemShowButtonShapes: true)
        let borders14 = NativeAppearancePolicy(macOSMajorVersion: 14, systemShowButtonShapes: true)
        try check(borders27.showButtonShapes && borders27.requiresButtonBorders, "macOS 27 show borders produces visible custom-control boundaries")
        try check(borders14.requiresButtonBorders, "back-deployed show borders preference also applies to macOS 14")
        try check(borders27.surfaceMode == .liquidGlass && !borders27.reduceTransparency, "show borders does not fabricate a transparency preference")
        try check(!mac27.requiresButtonBorders && !mac27.increaseContrast, "ordinary appearance does not fabricate accessibility preferences")

        // Exhaust all six independent preference flags across both fallback and
        // glass-era releases. This verifies the decision function, not rendered UI.
        var matrixCases = 0
        var matrixMatches = true
        for version in [14, 15, 26, 27] {
            for flags in 0..<64 {
                let appTransparency = flags & 1 != 0, systemTransparency = flags & 2 != 0
                let highContrast = flags & 4 != 0, appReduceMotion = flags & 8 != 0
                let systemReduceMotion = flags & 16 != 0, buttonShapes = flags & 32 != 0
                let policy = NativeAppearancePolicy(macOSMajorVersion: version, appReduceTransparency: appTransparency, systemReduceTransparency: systemTransparency, systemIncreaseContrast: highContrast, appReduceMotion: appReduceMotion, systemReduceMotion: systemReduceMotion, systemShowButtonShapes: buttonShapes)
                let opaque = appTransparency || systemTransparency || highContrast
                let motionReduced = appReduceMotion || systemReduceMotion
                let expectedSurface: NativeAppearancePolicy.SurfaceMode = opaque ? .opaque : (version >= 26 ? .liquidGlass : .vibrancy)
                matrixMatches = matrixMatches && policy.surfaceMode == expectedSurface
                    && policy.reduceTransparency == opaque && policy.reduceMotion == motionReduced
                    && policy.increaseContrast == highContrast && policy.showButtonShapes == buttonShapes
                    && policy.requiresButtonBorders == (highContrast || buttonShapes)
                    && policy.allowsAnimations == !motionReduced
                    && policy.allowsInteractiveGlass == (expectedSurface == .liquidGlass && !motionReduced)
                matrixCases += 1
            }
        }
        try check(matrixCases == 256 && matrixMatches, "all 256 native appearance preference/version combinations preserve accessibility precedence")
    }
    private static func settingsWindowLayoutTests() throws {
        let preferred = SettingsWindowLayout.preferredContentSize
        let visible = CGRect(x: 100, y: 80, width: 1440, height: 960)
        let shelfBefore = PanelLayout.frame(in: visible)
        let centered = SettingsWindowLayout.frame(in: visible, preferredSize: preferred)
        try check(preferred == CGSize(width: 710, height: 590), "independent settings preferred content size")
        try check(centered.midX == visible.midX && centered.midY == visible.midY && centered.size == preferred, "settings initially centered inside usable monitor")
        let moved = CGRect(x: 300, y: 150, width: 740, height: 620)
        try check(SettingsWindowLayout.frame(in: visible, preferredSize: preferred, previous: moved) == moved, "settings reopen preserves moved and resized frame")
        let removedScreenFrame = CGRect(x: -1600, y: 190, width: 710, height: 590)
        let restored = SettingsWindowLayout.frame(in: visible, preferredSize: preferred, previous: removedScreenFrame)
        try check(visible.contains(restored) && restored.minX == visible.minX && restored.minY == removedScreenFrame.minY, "unplugged secondary-screen settings clamp without recentering valid axis")
        let short = CGRect(x: -320, y: 40, width: 360, height: 220)
        try check(SettingsWindowLayout.frame(in: short, preferredSize: preferred) == short, "settings clamps to extremely short and narrow usable display")
        try check(SettingsWindowLayout.frame(in: short, preferredSize: preferred, previous: moved) == short, "restored large settings frame clamps both dimensions")
        try check(PanelLayout.frame(in: visible) == shelfBefore && shelfBefore.height == 328 && shelfBefore.minY == visible.minY, "settings geometry never changes bottom-shelf geometry")
        try check(SettingsWindowLayout.frame(in: CGRect(x: 0, y: 0, width: CGFloat.infinity, height: 900), preferredSize: preferred) == .zero, "invalid settings display geometry rejected")
    }
    @MainActor private static func hotkeyTests() throws {
        let sourceRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Sources")
        let recorderSource = try String(contentsOf: sourceRoot.appendingPathComponent("HotkeyRecorder.swift"), encoding: .utf8)
        try check(recorderSource.contains("guard let self else { return event }") && recorderSource.contains("return self.process(event)") && !recorderSource.contains("self?.process(event) ?? event"), "local monitor preserves nil consumption instead of reviving CmdQ or Escape")
        try check(recorderSource.contains("addLocalMonitorForEvents") && !recorderSource.contains("addGlobalMonitorForEvents"), "recorder installs no global keyboard observer")
        try check(recorderSource.contains("window.isKeyWindow, window.firstResponder === self") && recorderSource.contains("event.window == nil || event.window === window"), "local recorder gate requires its focused key window")
        try check(recorderSource.contains("pressGate.release(event.keyCode)") && recorderSource.contains("pressGate.owns(event.keyCode)"), "native recorder wires press-repeat and key-up suppression")
        try check(recorderSource.contains("NSWindow.didResignKeyNotification, NSWindow.willCloseNotification") && recorderSource.contains("override func resignFirstResponder()"), "native recorder cancels on focus and window lifecycle changes")
        let hotkeyViewSource = try String(contentsOf: sourceRoot.appendingPathComponent("HotkeySettingsView.swift"), encoding: .utf8)
        try check(hotkeyViewSource.contains(".accessibilityLabel(error)") && hotkeyViewSource.contains(".accessibilityAddTraits(.isStaticText)") && hotkeyViewSource.contains(".accessibilityIdentifier(\"hotkey-error\")"), "hotkey validation error explicitly exposes readable static text to accessibility")
        try check(hotkeyViewSource.contains("Label(error, systemImage: \"exclamationmark.triangle\")") && hotkeyViewSource.contains(".accessibilityElement(children: .ignore)") && !hotkeyViewSource.contains("textSelection"), "hotkey error uses native visible text without selectable-child accessibility merging")
        let standard = try HotkeySpec.parse(HotkeySpec.defaultValue)
        let alternate = try HotkeySpec.parse("control+option+k")
        try check(try HotkeySpec.parse(" ALT + CMD + V ").canonical == HotkeySpec.defaultValue, "hotkey aliases normalize to stable persistence format")
        try check(try HotkeySpec.parse("shift+ctrl+cmd+option+9").display == "⌃⌥⇧⌘9", "all modifier symbols use native display order")
        for invalid in ["", "v", "shift+v", "command+", "+command+v", "command++v", "command+cmd+v", "option+alt+v", "control+v+q", "command+中", "capslock+v", "command+space", "command+\n"] {
            do { _ = try HotkeySpec.parse(invalid); throw CoreError(code: "test", message: "invalid hotkey accepted") }
            catch let error as CoreError { try check(error.code == "hotkey", "invalid hotkey rejects ordinary typing, ambiguous or malformed components") }
        }
        var state = HotkeyRecordingState()
        try check(!state.receive(HotkeyStroke(keyCode: 12, modifiers: .command)) && state.draft == nil, "inactive recorder never intercepts CmdQ")
        state.begin()
        try check(state.receive(HotkeyStroke(keyCode: 9, modifiers: [])) && state.isRecording && state.draft == nil && state.error != nil, "unmodified input stays recording without a candidate")
        try check(state.receive(HotkeyStroke(keyCode: 9, modifiers: .shift)) && state.draft == nil, "Shift typing cannot become a global hotkey")
        state.updateModifiers([.option, .command])
        try check(state.heldModifiers.display == "⌥⌘", "recorder modifier feedback uses symbols")
        try check(state.receive(HotkeyStroke(keyCode: 9, modifiers: [.option, .command], isRepeat: true)) && state.isRecording, "autorepeat cannot choose a shortcut")
        try check(state.receive(HotkeyStroke(keyCode: 9, modifiers: [.option, .command])) && !state.isRecording && state.draft == HotkeySpec.defaultValue, "supported press creates an unsaved candidate only")
        state.begin(); _ = state.receive(HotkeyStroke(keyCode: 53, modifiers: []))
        try check(!state.isRecording && state.draft == HotkeySpec.defaultValue, "Escape cancels rerecording without discarding previous draft")
        state.begin()
        try check(!state.receive(HotkeyStroke(keyCode: 48, modifiers: .shift)) && !state.isRecording, "ShiftTab cancels recording and preserves native focus navigation")
        state.begin(); state.cancel()
        try check(!state.isRecording && state.draft == HotkeySpec.defaultValue, "focus-loss cancellation preserves prior draft")
        state.begin()
        try check(state.receive(HotkeyStroke(keyCode: 12, modifiers: .command)) && state.draft == "command+q", "CmdQ is consumed as input instead of passed to Quit")
        var pressGate = HotkeyPressGate()
        pressGate.consume(12)
        try check(pressGate.owns(12) && !state.isRecording, "recorded CmdQ remains consumed after candidate completes")
        try check(pressGate.owns(12) && !pressGate.owns(9), "held repeat suppression is limited to the consumed key")
        try check(pressGate.release(12) && pressGate.isEmpty && !pressGate.owns(12), "keyup ends menu-equivalent suppression")
        pressGate.consume(9); pressGate.reset()
        try check(pressGate.isEmpty, "focus loss releases local press suppression")
        state.begin(); _ = state.receive(HotkeyStroke(keyCode: 123, modifiers: .control))
        try check(state.isRecording && state.error != nil, "unsupported navigation key never commits")
        state.cancel(); state.didSave()
        try check(state.draft == nil && state.error == nil, "successful save resets editor state")

        var leases: Set<UInt32> = []
        var blocked: HotkeySpec?
        let plan = HotkeyRegistrationPlan(reserve: { proposed in
            if proposed.spec == blocked { throw CoreError(code: "hotkey_conflict", message: "Synthetic conflict") }
            leases.insert(proposed.id)
        }, release: { leases.remove($0.id) })
        try plan.register(standard)
        let original = plan.active!
        try plan.prepare(alternate)
        try check(plan.active == original && plan.pending?.spec == alternate && leases.count == 2, "prepare holds old and new registrations before persistence")
        try check(plan.spec(for: original.id) == standard && plan.spec(for: plan.pending!.id) == alternate, "registered-only Carbon routing recognizes both held combos")
        do { try plan.prepare(standard); throw CoreError(code: "test", message: "overlapping registration accepted") }
        catch let error as CoreError { try check(error.code == "hotkey_busy", "overlapping reservation rejected") }
        plan.finish(commit: false)
        try check(plan.active == original && plan.pending == nil && leases == [original.id], "rollback releases only proposal, never reacquires old combo")
        blocked = alternate
        do { try plan.prepare(alternate); throw CoreError(code: "test", message: "conflict accepted") }
        catch let error as CoreError { try check(error.code == "hotkey_conflict" && plan.active == original && !plan.updateInProgress && leases.count == 1, "conflict preserves active lease without transaction") }
        blocked = nil
        try plan.prepare(standard); plan.finish(commit: true)
        try check(plan.active == original && leases.count == 1, "saving an identical combo never duplicates or releases its lease")

        let engine = ManualEngine()
        let model = AppModel(engine: engine)
        model.settings = ["hotkey": HotkeySpec.defaultValue]
        var recordingChanges: [Bool] = []
        model.onHotkeyRecordingChanged = { recordingChanges.append($0) }
        model.onRegisterHotkey = { try plan.prepare(HotkeySpec.parse($0)) }
        model.onHotkeyRegistrationFinished = { plan.finish(commit: $0) }
        model.recordRegisteredHotkey("command+q")
        try check(model.hotkeyEditor.draft == nil, "registered combo outside focused recording is ignored")
        model.beginHotkeyRecording(); model.recordRegisteredHotkey(HotkeySpec.defaultValue)
        try check(recordingChanges == [true, false] && model.hotkeyEditor.draft == HotkeySpec.defaultValue && engine.requests.isEmpty, "Carbon reserved combo records without persistence or panel mutation")
        model.beginHotkeyRecording(); model.cancelHotkeyRecording()
        try check(recordingChanges == [true, false, true, false] && engine.requests.isEmpty, "cancel restores global action suspension and does not save")
        model.applyHotkey(alternate.canonical)
        try check(model.isApplyingHotkey && model.busy && model.hotkey == HotkeySpec.defaultValue && plan.active == original && leases.count == 2, "UI save reserves first and keeps old settings until core reply")
        model.applyHotkey("control+shift+j"); model.beginHotkeyRecording()
        try check(engine.requests.count == 1 && !model.isRecordingHotkey, "busy prevents duplicate saves and overlapping recording")
        model.refreshStatus()
        engine.reply(0, ["hotkey": alternate.canonical])
        try check(!model.isApplyingHotkey && !model.busy && model.hotkey == alternate.canonical && plan.active?.spec == alternate && leases.count == 1, "successful save commits the new lease and settings once")
        engine.reply(1, ["settings": ["hotkey": HotkeySpec.defaultValue], "sync_state": "disconnected"])
        try check(model.hotkey == alternate.canonical, "stale status begun during save cannot restore old shortcut")
        blocked = standard
        model.restoreDefaultHotkey()
        try check(engine.requests.count == 2 && model.hotkey == alternate.canonical && plan.active?.spec == alternate && model.hotkeyError?.contains("conflict") == true, "restore-default conflict cannot overwrite persisted or active shortcut")
        blocked = nil
        model.beginHotkeyRecording(); _ = model.recordHotkey(HotkeyStroke(keyCode: 9, modifiers: [.option, .command]))
        model.applyHotkey(HotkeySpec.defaultValue)
        engine.fail(2, CoreError(code: "storage", message: "Synthetic disk full"))
        try check(plan.active?.spec == alternate && leases.count == 1 && !plan.updateInProgress && model.hotkey == alternate.canonical, "failed persistence rolls back proposal with old lease continuously reserved")
        try check(model.hotkeyError?.contains("Synthetic disk full") == true && model.hotkeyEditor.draft == HotkeySpec.defaultValue && !model.busy, "failed save retains retryable draft and explicit error")
        model.restoreDefaultHotkey(); engine.reply(3, ["hotkey": HotkeySpec.defaultValue])
        try check(model.hotkey == HotkeySpec.defaultValue && plan.active?.spec == standard && model.hotkeyEditor.draft == nil && model.hotkeyError == nil, "restore default succeeds through the same checked transaction")
        model.patchSettings(["hotkey": "control+k", "appearance": "dark"])
        try check(engine.requests.count == 4, "mixed settings patch cannot bypass safe hotkey transaction")
        model.applyHotkey("shift+v")
        try check(engine.requests.count == 4 && model.hotkey == HotkeySpec.defaultValue, "invalid apply never touches core or registration")
        let noServiceEngine = ManualEngine()
        let noServiceModel = AppModel(engine: noServiceEngine)
        noServiceModel.applyHotkey(alternate.canonical)
        try check(noServiceEngine.requests.isEmpty && noServiceModel.hotkeyError != nil, "production refuses fake success without registration service")
        for demo in [false, true] {
            let isolatedEngine = ManualEngine(isDemo: demo)
            let isolated = AppModel(engine: isolatedEngine, systemIntegrationAllowed: false)
            var platformCalls = 0
            isolated.onRegisterHotkey = { _ in platformCalls += 1 }
            isolated.onHotkeyRegistrationFinished = { _ in platformCalls += 1 }
            isolated.onHotkeyRecordingChanged = { _ in platformCalls += 1 }
            isolated.beginHotkeyRecording(); _ = isolated.recordHotkey(HotkeyStroke(keyCode: 40, modifiers: [.control, .option]))
            isolated.applyHotkey(alternate.canonical); isolatedEngine.reply(0, ["hotkey": alternate.canonical])
            try check(platformCalls == 0 && isolated.hotkey == alternate.canonical && isolated.hotkeyNotice?.contains("未注册系统快捷键") == true, "demo and test can save synthetic shortcut without any platform callback")
        }
        plan.unregisterAll()
        try check(leases.isEmpty && plan.active == nil && plan.pending == nil, "registration shutdown releases controlled leases")
    }
    @MainActor private static func accessibilityPermissionTests() throws {
        var trusted = false, checkCount = 0, scheduled = 0, cancelled = 0
        var checkedTime = Date(timeIntervalSince1970: 1)
        var ticks: [@MainActor () -> Void] = []
        var intervals: [TimeInterval] = []
        let permission = AccessibilityPermission(enabled: true, check: { checkCount += 1; return trusted }, now: { checkedTime }, schedule: { interval, tick in
            scheduled += 1; intervals.append(interval); ticks.append(tick)
            return { cancelled += 1 }
        })
        try check(permission.state == .unchecked && checkCount == 0 && scheduled == 0, "permission model initialization neither checks trust nor starts a timer")
        permission.start()
        try check(permission.state == .notGranted && checkCount == 1 && permission.lastCheckedAt == checkedTime, "permission appearance reads the injected current-process result")
        try check(permission.isMonitoring && scheduled == 1 && intervals == [2], "visible permission page starts one periodic refresh")
        permission.start()
        try check(scheduled == 1 && checkCount == 1, "repeated appearance cannot duplicate permission timers")
        trusted = true; checkedTime = Date(timeIntervalSince1970: 2)
        permission.applicationDidBecomeActive()
        try check(permission.state == .granted && checkCount == 2 && permission.lastCheckedAt == checkedTime, "returning to foreground observes granted trust and refresh time")
        trusted = false; ticks[0]()
        try check(permission.state == .notGranted && checkCount == 3, "visible timer observes revoked trust instead of retaining a prior grant")
        trusted = true; permission.refresh()
        try check(permission.state == .granted && checkCount == 4, "manual permission check observes the latest result")
        permission.stop()
        try check(!permission.isMonitoring && cancelled == 1, "leaving permission page cancels the active timer")
        trusted = false; ticks[0](); permission.applicationDidBecomeActive()
        try check(checkCount == 4 && permission.state == .granted, "hidden permission page ignores queued timer and foreground callbacks")
        permission.start()
        try check(scheduled == 2 && checkCount == 5 && permission.state == .notGranted, "reopening permission page checks again and restarts its timer")
        trusted = true; ticks[0]()
        try check(checkCount == 5 && permission.state == .notGranted, "timer from an earlier appearance cannot update a reopened page")
        ticks[1]()
        try check(checkCount == 6 && permission.state == .granted, "current appearance timer still refreshes trust")
        var requests = 0, opened: [URL] = []
        trusted = false
        permission.requestAuthorization(request: { requests += 1 }, open: { opened.append($0); return true })
        try check(requests == 1 && opened == [AccessibilityPermission.settingsURL], "explicit authorization action requests prompt and opens only Accessibility settings")
        try check(permission.state == .notGranted && permission.settingsOpenError == nil && checkCount == 7, "opening system settings never fabricates a granted permission")
        permission.requestAuthorization(request: { requests += 1 }, open: { _ in false })
        try check(requests == 2 && permission.settingsOpenError?.contains("无法打开系统设置") == true && permission.state == .notGranted, "failed Settings launch is reported without claiming authorization")
        trusted = true; permission.refresh()
        try check(permission.state == .granted && requests == 2, "manual refresh can detect completed user authorization without another prompt")
        permission.stop(); permission.stop()
        try check(cancelled == 2 && !permission.isMonitoring, "permission timer cancellation is idempotent")

        var disabledChecks = 0, disabledTimers = 0, disabledRequests = 0, disabledOpens = 0
        let disabled = AccessibilityPermission(enabled: false, check: { disabledChecks += 1; return true }, schedule: { _, _ in disabledTimers += 1; return {} })
        disabled.start(); disabled.refresh(); disabled.applicationDidBecomeActive()
        disabled.requestAuthorization(request: { disabledRequests += 1 }, open: { _ in disabledOpens += 1; return true })
        disabled.stop()
        try check(disabledChecks == 0 && disabledTimers == 0 && disabledRequests == 0 && disabledOpens == 0, "synthetic permission model cannot inspect TCC, prompt, start polling, or open Settings")
        try check(disabled.state == .disabled && disabled.lastCheckedAt == nil && !disabled.isMonitoring, "synthetic permission model reports disabled rather than fabricated trust")
        var releasedTimer = 0
        weak var releasedPermission: AccessibilityPermission?
        do {
            let temporary = AccessibilityPermission(enabled: true, check: { false }, schedule: { _, _ in return { releasedTimer += 1 } })
            releasedPermission = temporary; temporary.start()
        }
        try check(releasedPermission == nil && releasedTimer == 1, "permission model destruction cancels any remaining timer")
        try check(AccessibilityPermission.State.notGranted.title == "当前版本未获系统授权" && AccessibilityPermission.State.granted.title == "当前版本已获系统授权", "permission copy identifies the running version rather than another listed app")

        let sourceRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Sources")
        let viewSource = try String(contentsOf: sourceRoot.appendingPathComponent("AccessibilityPermission.swift"), encoding: .utf8)
        let settingsSource = try String(contentsOf: sourceRoot.appendingPathComponent("SettingsView.swift"), encoding: .utf8)
        try check(settingsSource.contains("AccessibilitySettingsSection(model: model)") && !settingsSource.contains("AXIsProcessTrusted()"), "settings integrates observable permission state instead of a one-off body query")
        try check(viewSource.contains("enabled: !model.isSyntheticSession") && viewSource.contains("guard !model.isSyntheticSession else { return }") && viewSource.contains("guard enabled else { return }"), "permission view and model both guard synthetic system access")
        try check(viewSource.contains(".onAppear { permission.start() }") && viewSource.contains(".onDisappear { permission.stop() }") && viewSource.contains("NSApplication.didBecomeActiveNotification"), "permission view wires appearance, disappearance and app foreground lifecycle")
        try check(viewSource.contains("model.onPermissionRequest?()") && viewSource.contains("NSWorkspace.shared.open($0)") && !viewSource.contains("AXIsProcessTrustedWithOptions"), "only the explicit native action delegates permission prompting and opening Settings")
        try check(viewSource.contains("accessibility-permission-status") && viewSource.contains("accessibility-permission-refresh") && viewSource.contains(".accessibilityLabel(permission.state.title)"), "permission status and manual refresh expose stable accessible labels")
        try check(viewSource.contains("开发签名更新后") && viewSource.contains("/Applications/PasteLeaf.app") && viewSource.contains("退出并重新启动 PasteLeaf") && viewSource.contains("点击按钮不代表授权成功"), "permission recovery explains old-signature entries and preserves user-controlled authorization")
    }
    @MainActor static func main() async throws {
        try nativeAppearanceTests()
        try hotkeyTests()
        try settingsWindowLayoutTests()
        try accessibilityPermissionTests()
        let regular = LifecyclePolicy(arguments: [], environment: [:], bundleInfo: [:])
        try check(regular.mode == .regular && !regular.keepOpenForScreenshots, "regular launch defaults")
        try check(!LifecyclePolicy(arguments: ["--screenshot"], environment: [:], bundleInfo: [:]).keepOpenForScreenshots, "regular command line cannot disable blur hiding")
        try check(!LifecyclePolicy(arguments: [], environment: [:], bundleInfo: ["PLTestScreenshotMode": true]).keepOpenForScreenshots, "screenshot plist alone cannot alter regular lifecycle")
        let demoLifecycle = LifecyclePolicy(arguments: ["--demo", "--screenshot"], environment: [:], bundleInfo: [:])
        try check(demoLifecycle.mode == .demo && demoLifecycle.keepOpenForScreenshots, "explicit demo screenshot mode")
        let testLifecycle = LifecyclePolicy(arguments: [], environment: [:], bundleInfo: ["PLTestLibrary": true, "PLTestScreenshotMode": true])
        try check(testLifecycle.mode == .isolatedTest && testLifecycle.keepOpenForScreenshots, "test bundle screenshot mode")
        try check(!LifecyclePolicy(arguments: [], environment: ["PASTELEAF_TEST_MODE": "1"], bundleInfo: [:]).keepOpenForScreenshots, "test mode normally retains production blur semantics")
        try check(LifecyclePolicy(arguments: ["--demo"], environment: ["PASTELEAF_TEST_MODE": "1"], bundleInfo: [:]).mode == .isolatedTest, "isolated test safety wins conflicting demo flags")
        try check(regular.shouldHideOnResignKey(hasAttachedSheet: false, hasPresentedContent: false, hasModalWindow: false), "normal blur hides panel")
        try check(!regular.shouldHideOnResignKey(hasAttachedSheet: true, hasPresentedContent: false, hasModalWindow: false), "attached sheet does not hide its panel")
        try check(!regular.shouldHideOnResignKey(hasAttachedSheet: false, hasPresentedContent: true, hasModalWindow: false), "presented editor retains panel")
        try check(!regular.shouldHideOnResignKey(hasAttachedSheet: false, hasPresentedContent: false, hasModalWindow: true), "native modal picker retains panel")
        try check(!testLifecycle.shouldHideOnResignKey(hasAttachedSheet: false, hasPresentedContent: false, hasModalWindow: false), "isolated screenshot inspection stays open")
        try check(regular.reopenAction(panelExists: false, panelIsVisible: false, panelIsMiniaturized: false) == .awaitInitialLaunch, "reopen before launch never creates duplicate panel")
        try check(regular.reopenAction(panelExists: true, panelIsVisible: false, panelIsMiniaturized: false) == .showPanel, "reopen restores hidden panel")
        try check(regular.reopenAction(panelExists: true, panelIsVisible: true, panelIsMiniaturized: true) == .showPanel, "reopen restores miniaturized panel even if visible")
        try check(regular.reopenAction(panelExists: true, panelIsVisible: true, panelIsMiniaturized: false) == .focusExistingPanel, "reopen preserves active editor and selection")

        try check(PanelLayout.contentHeight == 328 && PanelLayout.contentHeight == DesignTokens.panelHeight, "all shelf rows and spacing sum to the shared compact panel height")
        try check(DesignTokens.cardHeight == 180 && DesignTokens.cardWidth == 240, "compact shelf preserves card width while lowering height")
        let wideScreen = CGRect(x: 0, y: 80, width: 3440, height: 1360)
        let widePanel = PanelLayout.frame(in: wideScreen)
        try check(widePanel.minX == wideScreen.minX && widePanel.maxX == wideScreen.maxX && widePanel.width == 3440, "wide monitor uses entire usable width without old 1450 cap")
        try check(widePanel.minY == 80 && widePanel.height == 328, "panel rests above reserved Dock area without bottom gap")
        let rightScreen = CGRect(x: 1920, y: 120, width: 2560, height: 1400)
        let rightPanel = PanelLayout.frame(in: rightScreen)
        try check(rightPanel == CGRect(x: 1920, y: 120, width: 2560, height: 328), "right-side offset monitor preserves its coordinate origin")
        let leftScreen = CGRect(x: -1680, y: -320, width: 1680, height: 1024)
        let leftPanel = PanelLayout.frame(in: leftScreen)
        try check(leftScreen.contains(leftPanel) && leftPanel.minX == -1680 && leftPanel.minY == -320, "negative-origin secondary display remains inside its own usable area")
        let upperScreen = CGRect(x: 240, y: 1080, width: 1920, height: 1056)
        try check(PanelLayout.frame(in: upperScreen).minY == 1080 && upperScreen.contains(PanelLayout.frame(in: upperScreen)), "vertically stacked display does not fall back to primary-screen coordinates")
        let narrowScreen = CGRect(x: 1400, y: 42, width: 320, height: 500)
        try check(PanelLayout.frame(in: narrowScreen) == CGRect(x: 1400, y: 42, width: 320, height: 328), "narrow screen has no negative width or side inset")
        let shortScreen = CGRect(x: -500, y: 24, width: 480, height: 220)
        try check(PanelLayout.frame(in: shortScreen) == shortScreen, "very short usable area clamps panel height instead of extending offscreen")
        try check(PanelLayout.needsVerticalScrolling(availableHeight: 220) && !PanelLayout.needsVerticalScrolling(availableHeight: 328), "short-screen shelf remains reachable through vertical scrolling")
        try check(PanelLayout.contentWidth(for: 320) == 280 && PanelLayout.contentWidth(for: 20) == 0, "internal content padding never produces a negative viewport")
        try check(PanelLayout.frame(in: CGRect(x: 5, y: 7, width: 0, height: 0)) == CGRect(x: 5, y: 7, width: 0, height: 0), "empty usable rectangle stays bounded")
        try check(PanelLayout.frame(in: CGRect(x: 0, y: 0, width: CGFloat.infinity, height: 900)) == .zero, "nonfinite screen geometry is not sent to AppKit")
        let fractionalScreen = CGRect(x: -1279.5, y: 67.5, width: 1279.5, height: 700.5)
        try check(PanelLayout.frame(in: fractionalScreen).minX == fractionalScreen.minX && PanelLayout.frame(in: fractionalScreen).maxX == fractionalScreen.maxX, "fractional logical coordinates keep both Retina edges aligned")

        let shortcut = try HotkeySpec.parse("option+command+v")
        try check(shortcut.display == "⌥⌘V", "default shortcut")
        try check(try HotkeySpec.parse("CTRL+SHIFT+9").display == "⌃⇧9", "shortcut case and aliases")
        do { _ = try HotkeySpec.parse("v"); throw CoreError(code: "test", message: "missing modifier accepted") } catch let error as CoreError { try check(error.code == "hotkey", "single key rejected") }
        do { _ = try HotkeySpec.parse("command+v+q"); throw CoreError(code: "test", message: "multiple keys accepted") } catch let error as CoreError { try check(error.code == "hotkey", "ambiguous shortcut rejected") }

        let largeID = "18446744073709551615"
        let decoded = try decodeCore(itemJSON(id: largeID, text: "合成中文记录"), as: ClipItem.self)
        try check(decoded.id == largeID, "IDs are never rounded through Double")
        try check(decoded.files.isEmpty && decoded.tags.isEmpty, "optional list payload defaults")
        try check(ClipItem(id: "url", text: "https://github.com/debugrao-coder/pasteleaf").isLink, "real HTTP URL type")
        try check(!ClipItem(id: "url", text: "javascript:alert(1)").isLink, "script URL not link preview")
        try check(!ClipItem(id: "url", text: "https://example.invalid\nsecond line").isLink, "multiline not link preview")
        try check(ClipItem(id: "image", kind: "image", text: "").footer == "图片", "image preview never reports zero text characters")
        try check(ClipItem(id: "image", kind: "image", text: "", available: false).footer == "等待附件下载", "image metadata retains pending attachment state")
        let first = ClipItem(id: "a", text: "甲")
        let second = ClipItem(id: "b", kind: "rich_text", text: "乙")
        if case .text(let text) = try PasteBatch.make([first, second]) { try check(text == "甲\n乙", "multi-text order and separator") } else { throw CoreError(code: "test", message: "multi-text batch wrong") }
        if case .sequence(let queue) = try PasteBatch.make([first, ClipItem(id: "pic", kind: "image", text: "")]) { try check(queue.count == 2, "mixed batch is explicit queue") } else { throw CoreError(code: "test", message: "mixed batch not queue") }
        do { _ = try PasteBatch.make([ClipItem(id: "image", kind: "image", text: "")], plain: true); throw CoreError(code: "test", message: "empty plain fallback accepted") } catch let error as CoreError { try check(error.code == "no_plain_text", "empty pure-text fallback cannot clear clipboard") }
        let file = ClipFile(name: "合成.txt", path: "/unused", size: 1, hash: "a")
        if case .files(let files) = try PasteBatch.make([ClipItem(id: "f1", kind: "files", text: "", files: [file]), ClipItem(id: "f2", kind: "files", text: "", files: [file])]) { try check(files.count == 2, "multi-files grouped") } else { throw CoreError(code: "test", message: "files batch not grouped") }
        do { _ = try PasteBatch.make([ClipItem(id: "waiting", text: "", available: false)]); throw CoreError(code: "test", message: "pending attachment accepted") } catch let error as CoreError { try check(error.code == "attachments_pending", "pending attachment blocked") }

        let offlineCopy = SyncStatusCopy.detail(state: "disconnected", raw: "Local history only; iCloud Drive is not connected", hasError: false, isDemo: false, isTestLibrary: false)
        try check(offlineCopy == "仅保留本机历史，尚未连接 iCloud Drive。", "normal sync explanation localized")
        try check(SyncStatusCopy.detail(state: "published", raw: "Written to the local sync folder; cloud delivery is not yet confirmed", hasError: false, isDemo: false, isTestLibrary: false).contains("尚未确认云端送达"), "localized publication never claims cloud delivery")
        try check(SyncStatusCopy.detail(state: "disconnected", raw: "unused", hasError: false, isDemo: false, isTestLibrary: true) == "隔离验证：不会连接 iCloud 或读取系统剪贴板。", "test copy explicitly names isolation")
        try check(SyncStatusCopy.detail(state: "error", raw: "synthetic disk-full diagnostic", hasError: true, isDemo: false, isTestLibrary: false).hasSuffix("synthetic disk-full diagnostic"), "actual error detail is not discarded by localization")
        try check(SyncStatusCopy.detail(state: "waiting_attachments", raw: "synthetic pending hydration", hasError: true, isDemo: false, isTestLibrary: true).contains("synthetic pending hydration"), "isolated label cannot hide real core failure")
        try check(SyncStatusCopy.detail(state: "pending", raw: "future diagnostic", hasError: false, isDemo: false, isTestLibrary: false).hasSuffix("future diagnostic"), "unknown explanatory detail remains visible")

        let longBody = String(repeating: "合成段落。", count: 180) + "末尾不同段落仍完整保留"
        let record: [String: Any] = ["id": "conflict", "kind": "text", "text": longBody, "tags": ["Mac设计"], "category": "collection", "source": "SyntheticEditor", "copied_at": 1_700_000_000_000, "available": true]
        let macVersion: [String: Any] = ["op_id": "mac-version", "device": "mac-fixture", "deleted": false, "record": record]
        var otherRecord = record; otherRecord["tags"] = ["Windows设计"]
        let windowsVersion: [String: Any] = ["op_id": "win-version", "device": "win-fixture", "deleted": false, "record": otherRecord]
        let conflict = try decodeCore(["entity": "item:conflict", "versions": [macVersion, windowsVersion]], as: ConflictGroup.self)
        try check(conflict.versions[0].tagsLabel == "#Mac设计" && conflict.versions[1].tagsLabel == "#Windows设计", "identical body conflicts show distinct tags")
        try check(conflict.versions[0].categoryLabel([ClipCategory(id: "collection", name: "设计资料", color: DesignTokens.categoryColors[0])]) == "设计资料", "conflict resolves category display name")
        try check(conflict.versions[0].categoryLabel([]).contains("collection"), "missing category identity remains available")
        try check(conflict.versions[0].record?.text == longBody && conflict.versions[0].bodyCharacterCount == longBody.count, "conflict detail preserves all body text beyond 180 characters")
        try check(conflict.versions[0].typeTitle == "文本" && conflict.versions[0].record?.source == "SyntheticEditor" && conflict.versions[0].capturedAtLabel != "未提供捕获时间", "conflict type source and capture time retained")
        let removed = try decodeCore(["op_id": "delete-version", "device": "fixture", "deleted": true], as: ConflictVersion.self)
        try check(removed.deleted && removed.statusTitle == "删除版本" && removed.record == nil, "deletion version is explicit without fabricated content")
        let categoryVersion = try decodeCore(["op_id": "category-version", "device": "fixture", "deleted": false, "category": ["id": "collection", "name": "设计资料新版", "color": DesignTokens.categoryColors[1]]], as: ConflictVersion.self)
        try check(categoryVersion.typeTitle == "收藏夹" && categoryVersion.category?.name == "设计资料新版" && categoryVersion.category?.color == DesignTokens.categoryColors[1], "category conflict shows name and color values")
        let hashA = String(repeating: "a", count: 64), hashB = String(repeating: "b", count: 64)
        let filesRecord: [String: Any] = ["id": "files-conflict", "kind": "files", "text": "", "files": [["name": "同名合成文件.txt", "size": 3, "hash": hashA, "path": "/must-not-read/a"], ["name": "同名合成文件.txt", "size": 3, "hash": hashB, "path": "/must-not-read/b"]]]
        let filesVersion = try decodeCore(["op_id": "files-version", "device": "fixture", "deleted": false, "record": filesRecord], as: ConflictVersion.self)
        try check(filesVersion.record?.files.map(\.hash) == [hashA, hashB] && filesVersion.record?.files.allSatisfy { $0.size == 3 && $0.name == "同名合成文件.txt" } == true, "same-name same-size attachments retain distinct full checksums without reading paths")
        try check(filesVersion.record?.files.first?.checksumDescription == "BLAKE3：" + hashA, "core attachment checksum is labeled BLAKE3 not SHA-256")
        let imageVersion = try decodeCore(["op_id": "image-version", "device": "fixture", "deleted": false, "record": ["id": "image-conflict", "kind": "image", "text": "", "image": ["name": "synthetic.png", "size": 1234, "hash": hashB], "image_path": "/must-not-read/image"]], as: ConflictVersion.self)
        try check(imageVersion.record?.image?.name == "synthetic.png" && imageVersion.record?.image?.size == 1234 && imageVersion.record?.image?.checksumDescription == "BLAKE3：" + hashB && imageVersion.record?.files.isEmpty == true, "image conflict preserves separate name bytes and BLAKE3 metadata")
        try check(imageVersion.record?.footer.hasPrefix("图片 · ") == true, "image preview includes actual size when core provides metadata")
        let digest = ConflictFormatDigest(format: "HTML", value: "abc")
        try check(digest.byteCount == 3 && digest.sha256 == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "format digest is byte-accurate SHA-256")
        let rtfA = Data([0x7b, 0x80, 0x7d]), rtfB = Data([0x7b, 0x81, 0x7d])
        try check(ConflictFormatDigest(format: "RTF", value: rtfA.base64EncodedString(), base64: true).sha256 != ConflictFormatDigest(format: "RTF", value: rtfB.base64EncodedString(), base64: true).sha256, "non-UTF8 RTF format changes remain distinguishable")
        try check(ConflictFormatDigest(format: "HTML", value: nil).summary.contains("未提供") && ConflictFormatDigest(format: "RTF", value: "invalid!", base64: true).invalid, "missing and invalid rich formats are explicit")
        let richVersion = try decodeCore(["op_id": "rich-version", "device": "fixture", "deleted": false, "record": ["id": "rich-conflict", "kind": "rich_text", "text": "same fallback", "html": "<img src='https://example.invalid/not-loaded'>", "rtf": rtfA.base64EncodedString()]], as: ConflictVersion.self)
        try check(richVersion.formats.count == 2 && richVersion.formats[1].byteCount == 3, "rich conflict keeps independent HTML and original RTF fingerprints without rendering")

        let manual = ManualEngine()
        let model = AppModel(engine: manual)
        try check(model.paused && !model.onboardingComplete, "onboarding never records by default")
        model.query = "old"; model.reload(force: true)
        model.query = "new"; model.reload(force: true)
        manual.reply(1, ["items": [itemJSON(id: "new", text: "最新结果")], "total": 1])
        manual.reply(0, ["items": [itemJSON(id: "old", text: "过期结果")], "total": 1])
        try check(model.items.map(\.id) == ["new"], "stale search cannot replace newer results")
        model.isPanelVisible = true; model.reload(force: false)
        manual.reply(2, ["items": [itemJSON(id: "arrival", text: "新到达"), itemJSON(id: "new", text: "最新结果")], "total": 2])
        try check(model.newItemsWaiting == 1 && model.items.map(\.id) == ["new"], "new arrivals preserve current card order")
        model.showNewItems()
        try check(model.items.map(\.id) == ["arrival", "new"], "new arrival explicit reveal")
        model.select("arrival"); model.select("new", modifiers: [.shift])
        try check(model.selectedIDs == ["arrival", "new"], "shift range selection")
        model.select("new", modifiers: [.command])
        try check(model.selectedIDs == ["arrival"], "command toggle selection")

        let rangeEngine = ManualEngine()
        let rangeModel = AppModel(engine: rangeEngine)
        let rangePage: [String: Any] = ["items": ["a", "b", "c", "d"].map { itemJSON(id: $0, text: "synthetic") }, "total": 4]
        rangeModel.reload(force: true); rangeEngine.reply(0, rangePage)
        rangeModel.moveSelection(1, extend: true)
        try check(rangeModel.selectedIDs == ["a", "b"], "first Shift Right includes automatically focused first card")
        rangeModel.moveSelection(1, extend: true); rangeModel.moveSelection(-1, extend: true)
        try check(rangeModel.selectedIDs == ["a", "b"], "reversing direction shrinks range without moving anchor")
        rangeModel.select("b"); rangeModel.moveSelection(1, extend: true); rangeModel.moveSelection(-1, extend: true); rangeModel.moveSelection(-1, extend: true)
        try check(rangeModel.selectedIDs == ["a", "b"], "range crosses anchor from right side to left")
        rangeModel.moveSelection(1, extend: true); rangeModel.moveSelection(1, extend: true)
        try check(rangeModel.selectedIDs == ["b", "c"], "crossing back preserves original user-selected anchor")
        rangeModel.reload(force: true); rangeEngine.reply(1, rangePage); rangeModel.moveSelection(-1, extend: true)
        try check(rangeModel.selectedIDs == ["b"], "ordinary refresh preserves valid range anchor")
        rangeModel.reload(force: true, resetSelection: true); rangeEngine.reply(2, rangePage); rangeModel.moveSelection(1, extend: true)
        try check(rangeModel.selectedIDs == ["a", "b"], "new search resets anchor to first result")
        rangeModel.select("c"); rangeModel.reload(force: true)
        rangeEngine.reply(3, ["items": ["a", "b", "d"].map { itemJSON(id: $0, text: "synthetic") }, "total": 3]); rangeModel.moveSelection(1, extend: true)
        try check(rangeModel.selectedIDs == ["a", "b"], "removed anchor is replaced with surviving focus")
        let unfocusedRange = AppModel(engine: ManualEngine())
        unfocusedRange.items = [first, second, ClipItem(id: "c", text: "丙")]; unfocusedRange.focusedID = "b"
        unfocusedRange.select("c", modifiers: [.shift])
        try check(unfocusedRange.selectedIDs == ["b", "c"], "missing anchor starts from previous focus before click moves it")

        let statusEngine = ManualEngine()
        let statusModel = AppModel(engine: statusEngine, systemIntegrationAllowed: false)
        statusModel.refreshStatus(); statusEngine.reply(0, ["sync_state": "disconnected", "detail": "Local history only; iCloud Drive is not connected", "error": NSNull()])
        try check(statusModel.syncDetail.hasPrefix("隔离验证"), "status adapter applies isolated explanation")
        statusModel.refreshStatus(); statusEngine.reply(1, ["sync_state": "error", "detail": "synthetic failure", "error": ["code": "fixture", "message": "synthetic failure"]])
        try check(statusModel.syncDetail.contains("synthetic failure"), "status adapter preserves core diagnostic under isolated label")

        let largeEngine = ManualEngine()
        let largeModel = AppModel(engine: largeEngine)
        largeModel.items = (0..<520).map { ClipItem(id: "large-\($0)", text: "fixture \($0)") }
        largeModel.reload(force: true)
        try check(largeEngine.requests[0].0["limit"] as? Int == 500, "refresh respects core page cap")
        largeEngine.reply(0, ["items": (0..<500).map { itemJSON(id: "large-\($0)", text: "fixture \($0)") }, "total": 600])
        try check(largeEngine.requests[1].0["offset"] as? Int == 500 && largeEngine.requests[1].0["limit"] as? Int == 20, "refresh requests continuation after 500")
        largeEngine.reply(1, ["items": (500..<520).map { itemJSON(id: "large-\($0)", text: "fixture \($0)") }, "total": 600])
        try check(largeModel.items.count == 520 && largeModel.total == 600, "refresh does not truncate permanent loaded history")

        let queueEngine = ManualEngine()
        let queueModel = AppModel(engine: queueEngine)
        queueModel.sequence = [first, second]
        queueModel.onPaste = { _, _ in false }
        queueModel.pasteNext()
        try check(queueEngine.requests[0].0["op"] as? String == "get", "queued paste re-verifies current item")
        queueEngine.reply(0, itemJSON(id: "a", text: "current fixture"))
        try check(queueModel.sequenceIndex == 0 && queueModel.sequence.count == 2, "failed clipboard write preserves queue")
        var writes = 0
        queueModel.onPaste = { _, _ in writes += 1; return true }
        queueModel.pasteNext(); queueEngine.reply(1, itemJSON(id: "a", text: "current fixture"))
        try check(queueModel.sequenceIndex == 1 && writes == 1, "queue consumes exactly one successful write")
        queueModel.pasteNext(); queueModel.sequence = []; queueModel.sequenceIndex = 0
        queueEngine.reply(2, itemJSON(id: "b", text: "cancelled fixture"))
        try check(writes == 1, "cancelled pending queue never writes clipboard")

        let safeEngine = ManualEngine()
        let safeModel = AppModel(engine: safeEngine, systemIntegrationAllowed: false)
        safeModel.command(["op": "sync_create", "directory": "/unused", "password": "synthetic"])
        try check(safeEngine.requests.isEmpty && safeModel.isTestLibrary, "isolated real-core GUI forbids cloud commands")
        safeModel.pasteSelection()
        try check(safeEngine.requests.isEmpty, "isolated real-core GUI forbids clipboard commands")
        do { try SafeImage.validate(Data("not an image".utf8)); throw CoreError(code: "test", message: "invalid image accepted") } catch let error as CoreError { try check(error.code == "image_size", "invalid image headers rejected before raster decode") }
        let fixture = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("Resources/demo-landscape.png")
        try check(SafeImage.pixelLimit == 16 * 1024 * 1024 && SafeImage.dimensionLimit == 16_384 && SafeImage.decodedByteLimit == 128 * 1024 * 1024, "native image ceilings match core")
        try SafeImage.validateDimensions(width: 4096, height: 4096, bitsPerComponent: 16)
        try check(true, "exact pixel and 128 MiB RGBA16 boundary accepted")
        do { try SafeImage.validateDimensions(width: 4096, height: 4097); throw CoreError(code: "test", message: "over-cap raster accepted") } catch let error as CoreError { try check(error.code == "image_size", "pixel overflow rejected before raster decode") }
        do { try SafeImage.validateDimensions(width: 16_385, height: 1); throw CoreError(code: "test", message: "over-cap dimension accepted") } catch let error as CoreError { try check(error.code == "image_size", "single-side overflow rejected") }
        do { try SafeImage.validateDimensions(width: 4096, height: 4096, bitsPerComponent: 32); throw CoreError(code: "test", message: "over-cap float buffer accepted") } catch let error as CoreError { try check(error.code == "image_size", "decoded buffer over 128 MiB rejected") }
        let safePreview = try SafeImage.previewSource(at: fixture)
        try check(CGImageSourceGetCount(safePreview) == 1, "preview validates same bounds before thumbnail decode")
        let fixtureData = try Data(contentsOf: fixture)
        try SafeImage.validate(fixtureData)
        let normalized = try SafeImage.pngData(fixtureData)
        try SafeImage.validate(normalized)
        try check(!normalized.isEmpty, "bounded synthetic image normalization")
        try check(try SafeImage.pasteboardType(normalized) == UTType.png.identifier, "PNG pasteboard uses PNG UTI")
        let jpeg = NSMutableData()
        let jpegSource = CGImageSourceCreateWithData(normalized as CFData, nil)!
        let jpegDestination = CGImageDestinationCreateWithData(jpeg, UTType.jpeg.identifier as CFString, 1, nil)!
        CGImageDestinationAddImageFromSource(jpegDestination, jpegSource, 0, nil)
        try check(CGImageDestinationFinalize(jpegDestination), "synthetic JPEG fixture generated")
        try check(try SafeImage.pasteboardType(jpeg as Data) == UTType.jpeg.identifier, "remote JPEG never mislabeled PNG")

        let demo = DemoEngine(now: 10_000_000)
        let demoStatus = try await request(demo, ["op": "status"]) as! [String: Any]
        try check(demo.isDemo && demoStatus["sync_state"] as? String == "disconnected", "demo has no fake synchronized state")
        do { _ = try await request(demo, ["op": "sync_create", "directory": "/unused", "password": "synthetic-only"]); throw CoreError(code: "test", message: "demo sync accepted") } catch let error as CoreError { try check(error.code == "demo_only", "demo rejects external mutations") }
        let demoPage = try decodeCore(try await request(demo, ["op": "list", "query": "", "offset": 0, "limit": 2]), as: ClipPage.self)
        try check(demoPage.items.count == 2 && demoPage.total == 8, "demo pagination matches contract")

        let root = FileManager.default.temporaryDirectory.appendingPathComponent("pasteleaf-mac-tests-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let real = RustEngine(root: root.appendingPathComponent("store"))
        let status = try await request(real, ["op": "status"]) as! [String: Any]
        let settings = status["settings"] as! [String: Any]
        try check(settings["paused"] as? Bool == true, "real engine fresh profile paused")
        _ = try await request(real, ["op": "settings", "values": ["onboarding_complete": true, "paused": false]])
        _ = try await request(real, ["op": "capture", "kind": "text", "text": "synthetic bridge fixture 中文", "source": "MacTests"])
        let rtfBytes = Data([0x7B, 0x5C, 0x72, 0x74, 0x66, 0x31, 0x20, 0x80, 0x7D])
        _ = try await request(real, ["op": "capture", "kind": "rich_text", "text": "synthetic rich fixture", "html": "<b>synthetic</b>", "rtf": rtfBytes.base64EncodedString(), "source": "MacTests"])
        let sourceFile = root.appendingPathComponent("synthetic.txt")
        try Data("synthetic attachment".utf8).write(to: sourceFile)
        _ = try await request(real, ["op": "capture", "kind": "files", "text": "synthetic file", "paths": [sourceFile.path], "source": "MacTests"])
        let page = try decodeCore(try await request(real, ["op": "list", "query": "", "category": NSNull(), "offset": 0, "limit": 50]), as: ClipPage.self)
        try check(page.total == 3 && page.items.count == 3, "real ABI capture and list")
        guard let rich = page.items.first(where: { $0.kind == "rich_text" }) else { throw CoreError(code: "test", message: "rich fixture missing") }
        let completeRich = try decodeCore(try await request(real, ["op": "get", "id": rich.id]), as: ClipItem.self)
        try check(Data(base64Encoded: completeRich.rtf ?? "") == rtfBytes, "RTF native byte roundtrip")
        guard let attachment = page.items.first(where: { $0.kind == "files" })?.files.first else { throw CoreError(code: "test", message: "attachment fixture missing") }
        try check(attachment.path != sourceFile.path && FileManager.default.fileExists(atPath: attachment.path), "attachment is copied to controlled storage")
        let filtered = try decodeCore(try await request(real, ["op": "list", "query": "中文", "offset": 0, "limit": 50]), as: ClipPage.self)
        try check(filtered.total == 1, "Chinese search through native bridge")
        let config = try await request(real, ["op": "settings"]) as! [String: Any]
        try check(config["onboarding_complete"] as? Bool == true, "onboarding consent persisted through ABI")
        let realHotkeyModel = AppModel(engine: real, systemIntegrationAllowed: false)
        realHotkeyModel.settings = config
        var actualPlatformCalls = 0
        realHotkeyModel.onRegisterHotkey = { _ in actualPlatformCalls += 1 }
        realHotkeyModel.onHotkeyRegistrationFinished = { _ in actualPlatformCalls += 1 }
        realHotkeyModel.applyHotkey("ctrl+alt+k")
        for _ in 0..<500 {
            if !realHotkeyModel.isApplyingHotkey { break }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        let persistedHotkey = try await request(real, ["op": "settings"]) as! [String: Any]
        try check(!realHotkeyModel.isApplyingHotkey && realHotkeyModel.hotkeyError == nil && actualPlatformCalls == 0 && persistedHotkey["hotkey"] as? String == "control+option+k", "real isolated ABI saves canonical hotkey without platform registration")
        realHotkeyModel.restoreDefaultHotkey()
        for _ in 0..<500 {
            if !realHotkeyModel.isApplyingHotkey { break }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        let restoredHotkey = try await request(real, ["op": "settings"]) as! [String: Any]
        try check(!realHotkeyModel.isApplyingHotkey && actualPlatformCalls == 0 && restoredHotkey["hotkey"] as? String == HotkeySpec.defaultValue, "real isolated ABI restores default through safe save path")
        print("PASS: \(count) macOS model / privacy / lifecycle / layout / appearance / stale-response / real ABI assertions. Synthetic isolated data only; no clipboard, hotkey, login, Keychain, or iCloud operations.")
    }
}
