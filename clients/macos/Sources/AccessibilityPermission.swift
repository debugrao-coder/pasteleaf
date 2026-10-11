import AppKit
import ApplicationServices
import Combine
import SwiftUI

/// Reports only the system's trust result for this running process. Checking never prompts.
/// Injected checks and timers let isolated tests exercise updates without accessing TCC.
@MainActor
final class AccessibilityPermission: ObservableObject {
    enum State: Equatable {
        case unchecked, granted, notGranted, disabled

        var title: String {
            switch self {
            case .unchecked: return "尚未检查"
            case .granted: return "当前版本已获系统授权"
            case .notGranted: return "当前版本未获系统授权"
            case .disabled: return "隔离模式：不检查系统权限"
            }
        }
    }
    typealias Scheduler = (TimeInterval, @escaping @MainActor () -> Void) -> (() -> Void)
    static let settingsURL = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!

    let enabled: Bool
    @Published private(set) var state: State
    @Published private(set) var lastCheckedAt: Date?
    @Published private(set) var settingsOpenError: String?
    private(set) var isMonitoring = false
    private let check: () -> Bool
    private let now: () -> Date
    private let schedule: Scheduler
    private var cancelTimer: (() -> Void)?
    private var generation: UInt = 0

    init(enabled: Bool, check: @escaping () -> Bool = { AXIsProcessTrusted() },
         now: @escaping () -> Date = Date.init, schedule: Scheduler? = nil) {
        self.enabled = enabled
        self.state = enabled ? .unchecked : .disabled
        self.check = check
        self.now = now
        self.schedule = schedule ?? Self.scheduleTimer
    }

    func start() {
        guard enabled, !isMonitoring else { return }
        isMonitoring = true
        generation &+= 1
        let currentGeneration = generation
        refresh()
        cancelTimer = schedule(2) { [weak self] in
            guard let self, self.isMonitoring, self.generation == currentGeneration else { return }
            self.refresh()
        }
    }

    func stop() {
        isMonitoring = false
        generation &+= 1
        cancelTimer?()
        cancelTimer = nil
    }

    func applicationDidBecomeActive() {
        guard isMonitoring else { return }
        refresh()
    }

    func refresh() {
        guard enabled else { return }
        state = check() ? .granted : .notGranted
        lastCheckedAt = now()
    }

    /// Opening Settings and requesting the system prompt do not grant permission.
    func requestAuthorization(request: () -> Void, open: (URL) -> Bool) {
        guard enabled else { return }
        settingsOpenError = nil
        request()
        if !open(Self.settingsURL) {
            settingsOpenError = "无法打开系统设置。请手动进入“系统设置 → 隐私与安全性 → 辅助功能”。"
        }
        refresh()
    }

    private static func scheduleTimer(_ interval: TimeInterval, _ tick: @escaping @MainActor () -> Void) -> (() -> Void) {
        let timer = Timer(timeInterval: interval, repeats: true) { _ in Task { @MainActor in tick() } }
        RunLoop.main.add(timer, forMode: .common)
        return { timer.invalidate() }
    }

    deinit { cancelTimer?() }
}

/// Native grouped-Form section, owned by the visible Settings page only.
@MainActor
struct AccessibilitySettingsSection: View {
    @ObservedObject var model: AppModel
    @StateObject private var permission: AccessibilityPermission

    init(model: AppModel) {
        self.model = model
        _permission = StateObject(wrappedValue: AccessibilityPermission(enabled: !model.isSyntheticSession))
    }

    var body: some View {
        Section {
            LabeledContent("辅助功能权限") {
                Text(permission.state.title).foregroundStyle(.secondary)
                    .accessibilityLabel(permission.state.title)
                    .accessibilityIdentifier("accessibility-permission-status")
            }
            HStack {
                Button("打开辅助功能设置…") {
                    guard !model.isSyntheticSession else { return }
                    permission.requestAuthorization(request: { model.onPermissionRequest?() }, open: { NSWorkspace.shared.open($0) })
                }
                .disabled(!permission.enabled)
                .accessibilityIdentifier("accessibility-permission-open-settings")
                Spacer()
                Button("重新检查") { permission.refresh() }
                    .disabled(!permission.enabled)
                    .accessibilityLabel("重新检查当前版本的辅助功能权限")
                    .accessibilityIdentifier("accessibility-permission-refresh")
            }
            if permission.state == .notGranted {
                Text("开发签名更新后，系统可能仍保留旧版本的授权。请在辅助功能设置中移除旧项，再添加 /Applications/PasteLeaf.app 并开启；若仍未更新，请退出并重新启动 PasteLeaf，然后重新检查。授权需由你在系统设置中完成。")
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            } else if permission.state == .disabled {
                Text("隔离模式不会查询或请求辅助功能权限，也不会打开系统设置。")
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            if let checked = permission.lastCheckedAt {
                Text("最近检查：\(checked.formatted(date: .omitted, time: .standard))")
                    .font(.caption).foregroundStyle(.secondary)
            }
            if let error = permission.settingsOpenError {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.callout).foregroundStyle(.red).fixedSize(horizontal: false, vertical: true)
                    .accessibilityElement(children: .ignore).accessibilityLabel(error)
                    .accessibilityAddTraits(.isStaticText)
            }
        } header: { Text("自动粘贴") } footer: {
            Text("以当前进程的系统检查结果为准；点击按钮不代表授权成功。授权后会验证原窗口再粘贴，未授权时仍可复制后手动粘贴。")
        }
        .onAppear { permission.start() }
        .onDisappear { permission.stop() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            permission.applicationDidBecomeActive()
        }
    }
}
