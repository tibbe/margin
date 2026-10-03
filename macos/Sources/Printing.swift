import AppKit

/// Prints the document as shown, not its source, without comments. Images
/// are printed once they have loaded (or failed to): printing waits for
/// them.
func printMarkdown(text: String, title: String, imageFolder: String, window: NSWindow) {
    let info = (NSPrintInfo.shared.copy() as! NSPrintInfo)
    info.horizontalPagination = .fit
    info.verticalPagination = .automatic
    info.isVerticallyCentered = false
    info.topMargin = 54
    info.bottomMargin = 54
    info.leftMargin = 54
    info.rightMargin = 54
    // Body text at 11pt on paper.
    let scale = 11 / Theme.bodySize
    info.scalingFactor = scale
    let width = (info.paperSize.width - info.leftMargin - info.rightMargin) / scale
    let view = printView(text: text, imageFolder: imageFolder, width: width)
    view.whenImagesSettled { [weak window] in
        guard let window else { return }
        view.sizeToFit()
        let op = NSPrintOperation(view: view, printInfo: info)
        op.jobTitle = title
        op.showsPrintPanel = true
        op.showsProgressPanel = true
        op.runModal(for: window, delegate: nil, didRun: nil, contextInfo: nil)
    }
}

/// The document laid out for paper, `width` wide, in the light
/// appearance, without comments or change marks.
func printView(text: String, imageFolder: String, width: CGFloat) -> DocView {
    let view = DocView.make()
    view.usesFullWidth = true
    view.isVerticallyResizable = true
    view.isContinuousSpellCheckingEnabled = false
    view.frame = NSRect(x: 0, y: 0, width: width, height: 100)
    view.textContainerInset = NSSize(width: 0, height: 0)
    view.backgroundColor = .white
    view.appearance = NSAppearance(named: .aqua)
    view.imageFolder = imageFolder
    view.setContents(text)
    view.updateGeometry(force: true)
    view.sizeToFit()
    return view
}
