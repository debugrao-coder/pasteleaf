import AppKit
import SwiftUI

extension Color {
    init(hex: String) {
        let value = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        let number = UInt64(value, radix: 16) ?? 0
        self.init(.sRGB, red: Double((number >> 16) & 255) / 255, green: Double((number >> 8) & 255) / 255, blue: Double(number & 255) / 255, opacity: 1)
    }
}

/// Semantic roles are generated from the shared token source, not a second Mac
/// palette. AppKit resolves them for appearance, contrast and system accent.
struct LeafTheme {
    init(_ scheme: ColorScheme) {}
    var background: Color { Color(nsColor: DesignTokens.MacSystemPalette.background) }
    var surface: Color { Color(nsColor: DesignTokens.MacSystemPalette.surface) }
    var elevated: Color { Color(nsColor: DesignTokens.MacSystemPalette.elevated) }
    var border: Color { Color(nsColor: DesignTokens.MacSystemPalette.border) }
    var text: Color { Color(nsColor: DesignTokens.MacSystemPalette.text) }
    var muted: Color { Color(nsColor: DesignTokens.MacSystemPalette.muted) }
    var accent: Color { .accentColor }
    var accentText: Color { Color(nsColor: .alternateSelectedControlTextColor) }
    var selection: Color { Color(nsColor: DesignTokens.MacSystemPalette.selection) }
    var danger: Color { Color(nsColor: DesignTokens.MacSystemPalette.danger) }
    var warning: Color { Color(nsColor: DesignTokens.MacSystemPalette.warning) }
}

private struct NativeAppearanceKey: EnvironmentKey {
    static let defaultValue = NativeAppearancePolicy(macOSMajorVersion: ProcessInfo.processInfo.operatingSystemVersion.majorVersion)
}
extension EnvironmentValues {
    var nativeAppearance: NativeAppearancePolicy {
        get { self[NativeAppearanceKey.self] }
        set { self[NativeAppearanceKey.self] = newValue }
    }
}

/// Ordinary forms and dialogs deliberately use system buttons, not glass.
struct LeafButtonStyle: PrimitiveButtonStyle {
    var prominent = false
    var danger = false
    @ViewBuilder func makeBody(configuration: Configuration) -> some View {
        if prominent {
            Button(configuration).buttonStyle(.borderedProminent).tint(danger ? Color.red : Color.accentColor)
        } else {
            Button(configuration).buttonStyle(.bordered).tint(danger ? Color.red : nil)
        }
    }
}

/// Glass belongs to the navigation/control layer, never clipboard content.
struct NativeToolbarButtonStyle: PrimitiveButtonStyle {
    @Environment(\.nativeAppearance) private var appearance
    var selected = false
    @ViewBuilder func makeBody(configuration: Configuration) -> some View {
        if #available(macOS 26.0, *), appearance.surfaceMode == .liquidGlass && !appearance.requiresButtonBorders {
            if selected && appearance.allowsInteractiveGlass { Button(configuration).buttonStyle(.glassProminent).controlSize(.regular) }
            else if selected { Button(configuration).buttonStyle(.borderedProminent).controlSize(.regular) }
            else { Button(configuration).buttonStyle(.glass(.regular.interactive(appearance.allowsInteractiveGlass))).tint(nil).controlSize(.regular) }
        } else if selected {
            Button(configuration).buttonStyle(.borderedProminent).controlSize(.regular)
        } else {
            Button(configuration).buttonStyle(.bordered).controlSize(.regular)
        }
    }
}

struct NativeControlGroup<Content: View>: View {
    @Environment(\.nativeAppearance) private var appearance
    @ViewBuilder var content: () -> Content
    @ViewBuilder var body: some View {
        if #available(macOS 26.0, *), appearance.surfaceMode == .liquidGlass {
            GlassEffectContainer(spacing: 8) { content() }
        } else { content() }
    }
}

struct IconButton: View {
    let symbol: String
    let title: String
    var action: () -> Void
    var body: some View {
        Button(action: action) {
            Image(systemName: symbol).font(.system(size: 14, weight: .medium)).frame(width: 16, height: 18)
        }.buttonStyle(NativeToolbarButtonStyle()).buttonBorderShape(.circle)
            .help(title).accessibilityLabel(title)
    }
}

struct VisualBackdrop: NSViewRepresentable {
    var disabled: Bool
    var scheme: ColorScheme
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = DesignTokens.macWindowMaterial
        view.blendingMode = .behindWindow
        view.state = .followsWindowActiveState
        return view
    }
    func updateNSView(_ view: NSVisualEffectView, context: Context) {
        view.isHidden = disabled
        view.appearance = NSAppearance(named: scheme == .dark ? .darkAqua : .aqua)
    }
}

struct StatusPill: View {
    @ObservedObject var model: AppModel
    var body: some View {
        Button {
            if model.conflictCount > 0 { model.loadConflicts() } else { model.settingsTab = "sync"; model.showSettings = true }
        } label: {
            Label(model.syncTitle, systemImage: model.syncSymbol).font(.system(size: 11, weight: .medium))
                .foregroundStyle(["error", "conflict"].contains(model.syncState) ? Color(nsColor: .systemOrange) : Color.primary)
                .lineLimit(1)
        }.buttonStyle(NativeToolbarButtonStyle()).buttonBorderShape(.capsule)
            .help(model.syncDetail).accessibilityLabel("同步状态：" + model.syncTitle)
    }
}

struct Keycap: View {
    var text: String
    var body: some View {
        Text(text).font(.system(size: 10, weight: .medium)).foregroundStyle(.secondary)
            .padding(.horizontal, 5).padding(.vertical, 2)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: DesignTokens.smallRadius))
    }
}

struct SectionHeading: View {
    var title: String
    var subtitle: String?
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.title3.weight(.semibold))
            if let subtitle { Text(subtitle).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
        }
    }
}

struct InfoBanner: View {
    var text: String
    var symbol = "info.circle"
    var warning = false
    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: symbol).font(.system(size: 15)).foregroundStyle(warning ? Color(nsColor: .systemOrange) : Color.secondary)
            Text(text).font(.system(size: 12)).foregroundStyle(.primary).lineSpacing(3).fixedSize(horizontal: false, vertical: true)
        }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: DesignTokens.sectionRadius))
    }
}
