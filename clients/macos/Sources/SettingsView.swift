import AppKit
import ApplicationServices
import SwiftUI

struct SettingsView: View {
    private static let tabHorizontalInset: CGFloat = 12
    @ObservedObject var model: AppModel
    var close: () -> Void
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.accessibilityShowBorders) private var showBorders
    @State private var exclusionInput = ""
    @State private var localGiB = "10"
    @State private var syncGiB = "10"
    @State private var fileMiB = "100"
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
        VStack(spacing: 0) {
            Picker("设置分类", selection: $model.settingsTab) {
                Text("通用").tag("general")
                Text("记录与存储").tag("capture")
                Text("iCloud 同步").tag("sync")
                Text("关于与迁移").tag("about")
            }
            .pickerStyle(.segmented).labelsHidden().accessibilityLabel("设置分类")
            .accessibilityIdentifier("settings-category")
            .frame(maxWidth: .infinity)
            .padding(.horizontal, Self.tabHorizontalInset).padding(.top, 16).padding(.bottom, 8)
            selectedSettings
            .frame(maxWidth: .infinity)
            .frame(minHeight: 0, maxHeight: .infinity)
            .padding(.horizontal, Self.tabHorizontalInset)
            if let error = model.errorMessage {
                NativeIssueMessage(text: error, dismiss: { model.errorMessage = nil })
                    .frame(maxHeight: 90).padding(.horizontal, 20).padding(.vertical, 8)
            }
            Divider()
            HStack(spacing: 8) {
                if model.busy {
                    ProgressView().controlSize(.small)
                    Text("正在处理…").font(.callout).foregroundStyle(.secondary)
                } else if let notice = model.notice {
                    Text(notice).font(.callout).foregroundStyle(.secondary).lineLimit(2).help(notice)
                }
                Spacer()
                Button("完成", action: close).keyboardShortcut(.cancelAction)
            }.padding(.horizontal, 20).padding(.vertical, 12)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .windowBackgroundColor))
        .modifier(LeafConfirmation(model: model))
        .preferredColorScheme(model.appearance == "dark" ? .dark : (model.appearance == "light" ? .light : nil))
        .environment(\.nativeAppearance, appearance)
        .transaction { if !appearance.allowsAnimations { $0.disablesAnimations = true; $0.animation = nil } }
        .onAppear {
            if !["general", "capture", "sync", "about"].contains(model.settingsTab) { model.settingsTab = "general" }
            exclusionInput = model.excludedApps.joined(separator: "\n")
            fileMiB = String((model.settings["max_file_bytes"] as? Int64 ?? 104_857_600) / 1_048_576)
            localGiB = String((model.settings["local_quota_bytes"] as? Int64 ?? 10_737_418_240) / 1_073_741_824)
            syncGiB = String((model.settings["sync_quota_bytes"] as? Int64 ?? 10_737_418_240) / 1_073_741_824)
        }
    }

    @ViewBuilder private var selectedSettings: some View {
        switch model.settingsTab {
        case "capture": capture
        case "sync": SyncSettingsView(model: model)
        case "about": about
        default: general
        }
    }

    private var general: some View {
        Form {
            Section("外观") {
                Picker("外观模式", selection: Binding(get: { model.appearance }, set: { model.patchSettings(["appearance": $0]) })) {
                    Text("跟随系统").tag("system"); Text("浅色").tag("light"); Text("深色").tag("dark")
                }
                Toggle("减少动画", isOn: settingBool("reduce_motion"))
                Toggle("关闭半透明背景", isOn: settingBool("reduce_transparency"))
            }
            Section {
                HotkeySettingsView(model: model)
                Toggle("登录后启动 PasteLeaf", isOn: Binding(get: { model.settings["launch_at_login"] as? Bool ?? false }, set: { model.onLoginChange?($0) }))
                    .disabled(model.isSyntheticSession)
            } header: { Text("快捷键与启动") } footer: {
                Text("发生冲突或保存失败时保留原快捷键，不抢占其他应用。")
            }
            AccessibilitySettingsSection(model: model)
            Section {
                Text("首次启用后不会自动读取之前的剪贴板。不会导入 Paste 历史，也不会修改 Paste 的快捷键、启动项或安装。")
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }.formStyle(.grouped)
    }

    private var capture: some View {
        Form {
            Section {
                Toggle("记录系统剪贴板", isOn: Binding(get: { !model.paused }, set: { enabled in
                    if enabled && !model.onboardingComplete { close(); model.onShowPanel?() } else { model.setPaused(!enabled) }
                })).disabled(model.isSyntheticSession)
            } header: { Text("记录范围") } footer: {
                Text("默认不排除敏感应用。密码、Token 和私密内容可能被记录；本地正文未加密，iCloud 传输文件使用独立同步密码加密。只记录系统允许读取的内容，不绕过系统权限或保护。")
            }
            Section {
                TextEditor(text: $exclusionInput).font(.system(.body, design: .monospaced)).frame(height: 88)
                    .accessibilityLabel("排除应用的 Bundle ID，每行一个")
                HStack {
                    Button("选择应用…") {
                        NativeFolders.pickApplication { id in
                            if let id { if !exclusionInput.isEmpty { exclusionInput += "\n" }; exclusionInput += id }
                        }
                    }.disabled(model.isSyntheticSession)
                    Spacer()
                    Button("保存排除列表") {
                        model.patchSettings(["excluded_apps": exclusionInput.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }])
                    }.disabled(model.isSyntheticSession || model.busy)
                }
            } header: { Text("不记录这些应用") } footer: { Text("每行一个应用 Bundle ID；只影响之后的复制。") }
            Section {
                quotaField("单个文件上限", value: $fileMiB, unit: "MiB")
                quotaField("本机目录上限", value: $localGiB, unit: "GiB")
                quotaField("同步目录上限", value: $syncGiB, unit: "GiB")
                Button("保存容量上限") { saveQuotas() }.disabled(model.isSyntheticSession || model.busy)
            } header: { Text("容量保护") } footer: {
                Text("不按时间或条数自动淘汰。达到保护上限后暂停新增，不会静默删除旧记录；iCloud 的实际可用配额仍由 Apple 管理。")
            }
            Section {
                Button("仅清空本机历史…", role: .destructive) { model.clearLocal() }.disabled(model.busy)
            } footer: {
                Text("只隐藏本机非收藏历史，不传播删除。收藏、附件和云端操作历史仍保留；这不是彻底擦除或释放磁盘空间。")
            }
        }.formStyle(.grouped)
    }

    private var about: some View {
        Form {
            Section("PasteLeaf") {
                LabeledContent("版本", value: appVersion)
                LabeledContent("客户端", value: "原生 macOS · Apple Silicon")
                LabeledContent("数据核心", value: "Rust · v2 加密同步")
                Link(destination: URL(string: "https://github.com/debugrao-coder/pasteleaf")!) {
                    Label("查看开源项目", systemImage: "arrow.up.right.square")
                }
            }
            Section {
                Text("iCloud Drive 是文件同步，不保证秒级送达。“已写入同步目录”不等于另一台设备已收到；只有加密导入回执到达后，才显示“对端已导入”。")
                    .fixedSize(horizontal: false, vertical: true)
            } header: { Text("同步状态的含义") }
            Section {
                Button("选择旧 PasteLeaf 数据目录…") {
                    NativeFolders.pick(title: "选择旧版 PasteLeaf 数据目录") { url in
                        guard let url else { return }
                        model.confirmation = .init(title: "导入旧版 PasteLeaf？", message: "将从所选目录备份并导入。原数据保持不变；请勿选择 Paste App 的目录。", actionTitle: "备份并导入") {
                            model.command(["op": "import_v1", "directory": url.path], message: "旧版数据导入已处理；请核对条目数量")
                        }
                    }
                }.disabled(model.isSyntheticSession || model.busy)
            } header: { Text("从旧版 PasteLeaf 导入") } footer: {
                Text("先备份再导入本地 JSON 历史，保留稳定标识。不会读取 Paste App 的数据，也不会覆盖或删除原目录。旧版和 v2 使用独立同步目录。")
            }
            Section {
                Text("本地开发版，未进行公开签名公证。支持 macOS 14+ / Apple Silicon。真实 Windows ↔ Mac iCloud 往返和辅助功能权限仍需在设备上单独验收。")
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }.formStyle(.grouped)
    }

    private func settingBool(_ key: String) -> Binding<Bool> {
        Binding(get: { model.settings[key] as? Bool ?? false }, set: { model.patchSettings([key: $0]) })
    }
    private var appVersion: String {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "未知版本"
        guard let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String, !build.isEmpty else { return version }
        return "\(version) (\(build))"
    }
    private func quotaField(_ title: String, value: Binding<String>, unit: String) -> some View {
        LabeledContent(title) {
            HStack {
                TextField(title, text: value).textFieldStyle(.roundedBorder).labelsHidden().frame(width: 80)
                    .multilineTextAlignment(.trailing).accessibilityLabel(title + "，单位 " + unit)
                Text(unit).foregroundStyle(.secondary).frame(width: 30, alignment: .leading)
            }
        }
    }
    private func saveQuotas() {
        guard let file = Int64(fileMiB), let local = Int64(localGiB), let sync = Int64(syncGiB), (1...100).contains(file), (1...10_240).contains(local), (1...10_240).contains(sync) else {
            model.fail(CoreError(code: "capacity", message: "请输入有效正整数：单个文件 1–100 MiB，目录 1–10240 GiB。")); return
        }
        model.patchSettings(["max_file_bytes": file * 1_048_576, "local_quota_bytes": local * 1_073_741_824, "sync_quota_bytes": sync * 1_073_741_824])
    }
}

struct SyncSettingsView: View {
    @ObservedObject var model: AppModel
    @State private var mode = "join"
    @StateObject private var connection: SyncConnectionDraft
    @State private var password = ""
    @State private var confirmPassword = ""
    @State private var remember = false

    init(model: AppModel) {
        self.model = model
        _connection = StateObject(wrappedValue: SyncConnectionDraft(engine: model.engine))
    }

    var body: some View {
        Form {
            Section {
                Label(model.syncTitle, systemImage: model.syncSymbol)
                Text(model.syncDetail).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            } header: { Text("同步状态") } footer: {
                Text("全量同步成功捕获的受支持内容，不覆盖另一台设备当前剪贴板。iCloud 文件到达时间由系统决定，请在两端选择同一个专用同步目录。")
            }
            if model.syncState == "disconnected" {
                Section("连接同步库") {
                    Picker("连接方式", selection: $mode) {
                        Text("加入已有同步库").tag("join"); Text("创建新同步库").tag("create")
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        Text("专用目录").font(.callout)
                        Text(connection.directory.isEmpty ? "尚未选择" : connection.directory)
                            .font(.system(.callout, design: .monospaced)).foregroundStyle(.secondary)
                            .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel("专用目录")
                    .accessibilityValue(connection.directory.isEmpty ? "尚未选择" : connection.directory)
                    HStack {
                        Button("选择 iCloud Drive 专用目录…") {
                            NativeFolders.pick(title: "选择 iCloud Drive 专用同步目录") { url in
                                guard let url else { return }
                                password = ""; confirmPassword = ""
                                connection.selectDirectory(url.path)
                            }
                        }.disabled(model.isSyntheticSession || model.busy)
                        if !connection.directory.isEmpty {
                            Button("重新检查目录") { connection.selectDirectory(connection.directory) }
                                .disabled(model.busy || connection.isChecking)
                        }
                    }
                    Label(connection.guidance, systemImage: connection.canJoin ? "checkmark.circle" : "info.circle")
                        .font(.callout).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityElement(children: .combine).accessibilityIdentifier("sync-directory-guidance")
                    passwordFields.disabled(mode == "create" ? !connection.canCreate : !connection.canJoin)
                    if mode == "create" {
                        PasswordEntry(title: "再次输入同步密码", text: $confirmPassword).disabled(!connection.canCreate)
                    }
                    Button(mode == "create" ? "创建并启用加密同步" : "解密并加入同步") { connect() }
                        .buttonStyle(.borderedProminent)
                        .disabled(model.isSyntheticSession || model.busy || password.isEmpty || (mode == "join" ? !connection.canJoin : !connection.canCreate || password != confirmPassword))
                }
                if mode == "create" {
                    Section { Text("请保管好同步密码；丢失后无法解密。创建不会向旧 v1 目录写入新格式。")
                        .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
                }
            } else if model.syncState == "locked" {
                Section("解锁同步库") {
                    if !model.syncDirectory.isEmpty { directoryRow }
                    passwordFields
                    Button("解锁同步库") {
                        let command: [String: Any] = ["op": "sync_unlock", "password": password, "remember": remember]
                        password = ""
                        model.command(command, message: "已请求解锁同步")
                    }.buttonStyle(.borderedProminent).disabled(model.isSyntheticSession || model.busy || password.isEmpty)
                }
                Section { disconnectButton }
            } else {
                Section("同步详情") {
                    if !model.syncDirectory.isEmpty { directoryRow }
                    LabeledContent("待发布") { Text(model.pending.formatted()).monospacedDigit() }
                    LabeledContent("导入回执") { Text(model.peerReceipts.formatted()).monospacedDigit() }
                    LabeledContent("冲突") { Text(model.conflictCount.formatted()).monospacedDigit() }
                    if let time = model.lastExchange { LabeledContent("最近交换", value: time.formatted(date: .abbreviated, time: .standard)) }
                }
                Section {
                    HStack {
                        Button("立即检查") { model.command(["op": "sync_now"], message: "已检查同步目录；到达状态以回执为准") }
                            .disabled(model.busy || model.isSyntheticSession)
                        Button(model.syncState == "paused" ? "恢复同步" : "暂停同步") {
                            model.command(["op": "sync_pause", "paused": model.syncState != "paused"])
                        }.disabled(model.busy || model.isSyntheticSession)
                        if model.conflictCount > 0 {
                            Button("处理冲突…") {
                                model.showSettings = false
                                DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { model.onShowPanel?(); model.loadConflicts() }
                            }
                        }
                    }
                    disconnectButton
                }
            }
            Section {
                Text("选择“记住”后，仅将派生的同步密钥保存在 Keychain，不保存原始密码。断开连接会忘记本机密钥，但不会删除本地历史或云端文件。")
                    .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }.formStyle(.grouped)
            .onChange(of: mode) { _, _ in password = ""; confirmPassword = "" }
            .onDisappear { password = ""; confirmPassword = "" }
    }
    private var directoryRow: some View {
        LabeledContent("同步目录") {
            Text(model.syncDirectory).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                .font(.system(.callout, design: .monospaced))
        }
    }
    @ViewBuilder private var passwordFields: some View {
        PasswordEntry(title: "同步密码（不是 Apple ID 密码）", text: $password)
        Toggle("在这台 Mac 的 Keychain 记住密钥", isOn: $remember).toggleStyle(.checkbox)
    }
    private var disconnectButton: some View {
        Button("断开同步并忘记本机密钥…", role: .destructive) {
            model.confirmation = .init(title: "断开 iCloud 同步？", message: "停止这台 Mac 的同步并移除记住的密钥；不会删除本地记录或云端历史。", actionTitle: "断开并忘记密钥") {
                model.command(["op": "sync_disconnect"], message: "已断开同步并忘记本机密钥")
            }
        }.disabled(model.busy || model.isSyntheticSession)
    }
    private func connect() {
        guard mode == "create" ? connection.canCreate : connection.canJoin else { return }
        guard mode != "create" || password == confirmPassword else { model.fail(CoreError(code: "password_mismatch", message: "两次输入的同步密码不一致。")); return }
        let command: [String: Any] = ["op": mode == "create" ? "sync_create" : "sync_join", "directory": connection.directory, "password": password, "remember": remember]
        // Clear visible drafts even when authentication or I/O fails.
        password = ""; confirmPassword = ""
        model.command(command, message: "同步配置已保存；真实到达状态以导入回执为准")
    }
}

struct PasswordEntry: View {
    let title: String
    @Binding var text: String
    @State private var revealed = false
    @FocusState private var fieldFocused: Bool
    var body: some View {
        LabeledContent(title) {
            HStack(spacing: 8) {
                Group {
                    if revealed { TextField("同步密码", text: $text) }
                    else { SecureField("同步密码", text: $text) }
                }.labelsHidden().textFieldStyle(.roundedBorder).textContentType(.password)
                    .accessibilityLabel(title).focused($fieldFocused)
                Button {
                    let restoreFocus = fieldFocused
                    revealed.toggle()
                    if restoreFocus { DispatchQueue.main.async { fieldFocused = true } }
                } label: { Image(systemName: revealed ? "eye.slash" : "eye") }
                    .buttonStyle(.borderless).help(revealed ? "隐藏同步密码" : "显示同步密码")
                    .accessibilityLabel(revealed ? "隐藏同步密码" : "显示同步密码")
                    .accessibilityValue(revealed ? "当前显示" : "当前隐藏")
            }.frame(maxWidth: 280)
        }.onChange(of: text.isEmpty) { _, empty in if empty { revealed = false } }
            .onDisappear { revealed = false }
    }
}
