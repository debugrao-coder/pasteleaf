import CoreGraphics

enum SettingsWindowLayout {
    static let preferredContentSize = CGSize(width: 710, height: 590)

    /// Restore only the settings frame. Clamp it after a display is unplugged or resized.
    static func frame(in visible: CGRect, preferredSize: CGSize, previous: CGRect? = nil) -> CGRect {
        guard visible.origin.x.isFinite, visible.origin.y.isFinite,
              visible.width.isFinite, visible.height.isFinite,
              preferredSize.width.isFinite, preferredSize.height.isFinite else { return .zero }
        let requested = previous ?? CGRect(origin: .zero, size: preferredSize)
        let width = min(max(0, requested.width), max(0, visible.width))
        let height = min(max(0, requested.height), max(0, visible.height))
        let x = previous?.minX ?? (visible.midX - width / 2)
        let y = previous?.minY ?? (visible.midY - height / 2)
        return CGRect(x: min(max(x, visible.minX), visible.maxX - width),
                      y: min(max(y, visible.minY), visible.maxY - height), width: width, height: height)
    }
}
