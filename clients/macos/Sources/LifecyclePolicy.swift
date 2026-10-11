import Foundation

/// Pure launch/window policy. UI inspection flags never alter a regular recording session.
struct LifecyclePolicy {
    enum SessionMode: Equatable { case regular, demo, isolatedTest }
    enum ReopenAction: Equatable { case awaitInitialLaunch, showPanel, focusExistingPanel }
    let mode: SessionMode
    let keepOpenForScreenshots: Bool

    init(arguments: [String], environment: [String: String], bundleInfo: [String: Any]) {
        if arguments.contains("--test-library") || environment["PASTELEAF_TEST_MODE"] == "1" || bundleInfo["PLTestLibrary"] as? Bool == true {
            mode = .isolatedTest
        } else if arguments.contains("--demo") || environment["PASTELEAF_DEMO"] == "1" || bundleInfo["PLDemo"] as? Bool == true {
            mode = .demo
        } else { mode = .regular }
        let requested = arguments.contains("--screenshot") || bundleInfo["PLTestScreenshotMode"] as? Bool == true
        keepOpenForScreenshots = mode != .regular && requested
    }
    func shouldHideOnResignKey(hasAttachedSheet: Bool, hasPresentedContent: Bool, hasModalWindow: Bool) -> Bool {
        !keepOpenForScreenshots && !hasAttachedSheet && !hasPresentedContent && !hasModalWindow
    }
    func reopenAction(panelExists: Bool, panelIsVisible: Bool, panelIsMiniaturized: Bool) -> ReopenAction {
        guard panelExists else { return .awaitInitialLaunch }
        return !panelIsVisible || panelIsMiniaturized ? .showPanel : .focusExistingPanel
    }
}
