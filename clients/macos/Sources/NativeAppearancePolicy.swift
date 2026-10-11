/// Pure rendering decisions. Callers provide snapshots of app and accessibility
/// preferences; this type never reads or writes settings or platform state.
///
/// Liquid Glass APIs require macOS 26. A selected mode does not replace the
/// caller's `if #available(macOS 26.0, *)` guard around those APIs.
struct NativeAppearancePolicy: Equatable, Sendable {
    enum SurfaceMode: Equatable, Sendable {
        case liquidGlass
        case vibrancy
        case opaque
    }

    let surfaceMode: SurfaceMode
    let reduceTransparency: Bool
    let reduceMotion: Bool
    let increaseContrast: Bool
    let showButtonShapes: Bool

    init(
        macOSMajorVersion: Int,
        appReduceTransparency: Bool = false,
        systemReduceTransparency: Bool = false,
        systemIncreaseContrast: Bool = false,
        appReduceMotion: Bool = false,
        systemReduceMotion: Bool = false,
        systemShowButtonShapes: Bool = false
    ) {
        reduceTransparency = appReduceTransparency || systemReduceTransparency || systemIncreaseContrast
        reduceMotion = appReduceMotion || systemReduceMotion
        increaseContrast = systemIncreaseContrast
        // SDK 27 exposes the preferred SwiftUI name accessibilityShowBorders
        // (back-deployed), previously accessibilityShowButtonShapes. The intent
        // applies on supported older macOS versions too, not only macOS 27.
        showButtonShapes = systemShowButtonShapes
        if reduceTransparency || macOSMajorVersion < 14 {
            surfaceMode = .opaque
        } else if macOSMajorVersion >= 26 {
            surfaceMode = .liquidGlass
        } else {
            surfaceMode = .vibrancy
        }
    }

    var requiresButtonBorders: Bool { increaseContrast || showButtonShapes }
    var allowsAnimations: Bool { !reduceMotion }
    var allowsInteractiveGlass: Bool { surfaceMode == .liquidGlass && !reduceMotion }
}
