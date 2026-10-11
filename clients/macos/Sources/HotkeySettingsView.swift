import SwiftUI

/// Embeddable native settings rows. Domain persistence and registration belong to AppModel.
struct HotkeySettingsView: View {
    @ObservedObject var model: AppModel
    var body: some View {
        LabeledContent("呼出剪贴板") {
            HStack(spacing: 8) {
                HotkeyRecorder(model: model).frame(width: 190, height: 28)
                Button("保存") {
                    if let draft = model.hotkeyEditor.draft { model.applyHotkey(draft) }
                }.disabled(model.hotkeyEditor.draft == nil || model.isRecordingHotkey || model.busy)
                    .accessibilityLabel("保存新的全局快捷键")
            }
        }
        HStack {
            if model.isRecordingHotkey {
                Button("取消录入") { model.cancelHotkeyRecording() }
            } else {
                Button("恢复默认 ⌥⌘V") { model.restoreDefaultHotkey() }.disabled(model.busy)
            }
            Spacer()
            if model.isApplyingHotkey {
                ProgressView().controlSize(.small)
                Text("正在检查并保存…").font(.callout).foregroundStyle(.secondary)
            }
        }
        Text("点按快捷键后按下新组合，再点“保存”。至少包含 ⌃、⌥ 或 ⌘；Esc 取消，Tab 或失焦退出录入。")
            .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        if model.isSyntheticSession {
            Text(model.isTestLibrary ? "仅测试库：可录入并保存设置，但不会注册或更改任何系统快捷键。" : "仅演示：可试用录入与保存，不会注册系统快捷键。")
                .font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
        if let error = model.hotkeyEditor.error ?? model.hotkeyError {
            Label(error, systemImage: "exclamationmark.triangle")
                .font(.callout).foregroundStyle(.red)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(error)
                .accessibilityAddTraits(.isStaticText)
                .accessibilityIdentifier("hotkey-error")
        } else if let notice = model.hotkeyNotice {
            Label(notice, systemImage: "checkmark.circle").font(.callout).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
