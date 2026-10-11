import CoreGraphics

/// Screen geometry is shared by initial creation and every subsequent reveal.
/// AppKit supplies visibleFrame, so the shelf never reserves or covers Dock space.
enum PanelLayout {
    static var contentHeight: CGFloat {
        DesignTokens.panelTopPadding + DesignTokens.headerHeight + DesignTokens.headerGap
            + DesignTokens.categoryHeight + DesignTokens.categoryGap
            + DesignTokens.cardHeight + DesignTokens.cardStripExtra
            + DesignTokens.footerHeight + DesignTokens.footerGap + DesignTokens.panelBottomPadding
    }
    static func frame(in visibleFrame: CGRect) -> CGRect {
        guard visibleFrame.origin.x.isFinite, visibleFrame.origin.y.isFinite,
              visibleFrame.width.isFinite, visibleFrame.height.isFinite else { return .zero }
        let width = max(0, visibleFrame.width)
        let availableHeight = max(0, visibleFrame.height)
        let bottomInset = min(max(0, DesignTokens.panelBottomInset), availableHeight)
        let height = min(max(0, DesignTokens.panelHeight), availableHeight - bottomInset)
        return CGRect(x: visibleFrame.minX, y: visibleFrame.minY + bottomInset, width: width, height: height)
    }
    static func contentWidth(for panelWidth: CGFloat) -> CGFloat {
        max(0, panelWidth - DesignTokens.panelPadding * 2)
    }
    static func needsVerticalScrolling(availableHeight: CGFloat) -> Bool { availableHeight < contentHeight }
}
