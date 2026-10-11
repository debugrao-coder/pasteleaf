import Foundation

struct HotkeyModifiers: OptionSet, Equatable {
    let rawValue: UInt8
    static let control = Self(rawValue: 1 << 0)
    static let option = Self(rawValue: 1 << 1)
    static let shift = Self(rawValue: 1 << 2)
    static let command = Self(rawValue: 1 << 3)
    var names: [String] {
        [(Self.control, "control"), (.option, "option"), (.shift, "shift"), (.command, "command")]
            .compactMap { contains($0.0) ? $0.1 : nil }
    }
    var display: String {
        [(Self.control, "⌃"), (.option, "⌥"), (.shift, "⇧"), (.command, "⌘")]
            .compactMap { contains($0.0) ? $0.1 : nil }.joined()
    }
}

struct HotkeyStroke {
    let keyCode: UInt16
    let modifiers: HotkeyModifiers
    var isRepeat = false
    var hasUnsupportedModifier = false
}

/// Consume the full press, including repeats and key-up, after a candidate ends recording.
/// This prevents a held Cmd+Q from falling through to the app menu on its next repeat.
struct HotkeyPressGate {
    private var consumed: Set<UInt16> = []
    var isEmpty: Bool { consumed.isEmpty }
    func owns(_ keyCode: UInt16) -> Bool { consumed.contains(keyCode) }
    mutating func consume(_ keyCode: UInt16) { consumed.insert(keyCode) }
    @discardableResult mutating func release(_ keyCode: UInt16) -> Bool { consumed.remove(keyCode) != nil }
    mutating func reset() { consumed = [] }
}

/// Only an explicitly focused recorder invokes this reducer. No text or clipboard is read.
struct HotkeyRecordingState {
    private(set) var isRecording = false
    private(set) var draft: String?
    private(set) var error: String?
    private(set) var heldModifiers: HotkeyModifiers = []
    private var priorDraft: String?

    mutating func begin() {
        guard !isRecording else { return }
        priorDraft = draft; isRecording = true; error = nil; heldModifiers = []
    }
    mutating func cancel() {
        guard isRecording else { return }
        draft = priorDraft; priorDraft = nil; isRecording = false; error = nil; heldModifiers = []
    }
    mutating func updateModifiers(_ value: HotkeyModifiers) { if isRecording { heldModifiers = value } }
    mutating func accept(_ value: String) {
        guard isRecording else { return }
        do {
            draft = try HotkeySpec.parse(value).canonical
            priorDraft = nil; isRecording = false; error = nil; heldModifiers = []
        } catch { self.error = error.localizedDescription }
    }
    /// True consumes the key before menu equivalents. Tab cancels and retains native navigation.
    mutating func receive(_ stroke: HotkeyStroke) -> Bool {
        guard isRecording else { return false }
        if stroke.keyCode == 53 { cancel(); return true }
        if stroke.keyCode == 48 { cancel(); return false }
        guard !stroke.isRepeat else { return true }
        guard !stroke.hasUnsupportedModifier, let key = HotkeySpec.keyCodes.first(where: { $0.value == UInt32(stroke.keyCode) })?.key else {
            error = "暂不支持这个按键；请使用英文字母或数字。按 Esc 取消。"; return true
        }
        accept((stroke.modifiers.names + [key]).joined(separator: "+"))
        return true
    }
    mutating func didSave() { draft = nil; priorDraft = nil; isRecording = false; error = nil; heldModifiers = [] }
}

/// Resource-independent two-phase registration; a failed save never releases the old lease.
@MainActor
final class HotkeyRegistrationPlan {
    struct Registration: Equatable { let id: UInt32; let spec: HotkeySpec }
    private(set) var active: Registration?
    private(set) var pending: Registration?
    private(set) var updateInProgress = false
    private var nextID: UInt32 = 1
    private let reserve: (Registration) throws -> Void
    private let release: (Registration) -> Void
    init(reserve: @escaping (Registration) throws -> Void, release: @escaping (Registration) -> Void) {
        self.reserve = reserve; self.release = release
    }
    func prepare(_ spec: HotkeySpec) throws {
        guard !updateInProgress else { throw CoreError(code: "hotkey_busy", message: "正在保存快捷键，请稍候。") }
        if active?.spec == spec { updateInProgress = true; return }
        while nextID == 0 || nextID == active?.id || nextID == pending?.id { nextID &+= 1 }
        let proposed = Registration(id: nextID, spec: spec); nextID &+= 1
        try reserve(proposed)
        pending = proposed; updateInProgress = true
    }
    func finish(commit: Bool) {
        guard updateInProgress else { return }
        if let pending {
            if commit {
                if let active { release(active) }
                active = pending
            } else { release(pending) }
        }
        pending = nil; updateInProgress = false
    }
    func register(_ spec: HotkeySpec) throws {
        if active?.spec == spec { return }
        try prepare(spec); finish(commit: true)
    }
    func unregisterAll() {
        finish(commit: false)
        if let active { release(active) }; active = nil
    }
    func spec(for id: UInt32) -> HotkeySpec? {
        if active?.id == id { return active?.spec }
        return pending?.id == id ? pending?.spec : nil
    }
}
