import XCTest

/// Margin driven from outside, as a person drives it: real keys and clicks
/// through the system. These take the keyboard and mouse while they run.
final class MarginUITests: XCTestCase {
    /// Launches Margin with a data directory of its own, so it neither
    /// reopens nor touches the windows, drafts or comments a person has, and
    /// starts a new document. (The runner is sandboxed, so the two can't
    /// share files it makes.)
    @MainActor
    private func launch() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["MARGIN_DATA_DIR"] = "/tmp/MarginUITests-\(UUID().uuidString)"
        app.launchArguments += ["-ApplePersistenceIgnoreState", "YES", "-NSQuitAlwaysKeepsWindows", "NO"]
        app.launch()
        addTeardownBlock { @MainActor in app.terminate() }
        // With nothing to reopen, launching shows the Open panel.
        let panel = app.windows["open-panel"]
        XCTAssertTrue(panel.waitForExistence(timeout: 5))
        panel.buttons["Cancel"].click()
        app.typeKey("n", modifierFlags: .command)
        XCTAssertTrue(window(app).waitForExistence(timeout: 5), app.windows.debugDescription)
        return app
    }

    /// The document's window.
    @MainActor
    private func window(_ app: XCUIApplication) -> XCUIElement {
        app.windows.matching(identifier: "document").firstMatch
    }

    @MainActor
    private func text(_ app: XCUIApplication) -> String {
        window(app).textViews.firstMatch.value as? String ?? ""
    }

    @MainActor
    private func wait(for text: String, in app: XCUIApplication, file: StaticString = #filePath, line: UInt = #line) {
        let deadline = Date(timeIntervalSinceNow: 5)
        while self.text(app) != text && Date() < deadline {
            RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
        }
        XCTAssertEqual(self.text(app), text, file: file, line: line)
    }

    /// Keys go through the keyboard layout, dead keys included, as a
    /// person types them. Assumes the ABC (or US) layout.
    @MainActor
    func testKeysGoThroughTheKeyboardLayout() {
        let app = launch()
        app.typeText("x")
        // Option-E, then E: é.
        app.typeKey("e", modifierFlags: .option)
        app.typeKey("e", modifierFlags: [])
        wait(for: "xé", in: app)
        // Shift-Return: a line break within the paragraph.
        app.typeKey(.enter, modifierFlags: .shift)
        app.typeKey("b", modifierFlags: [])
        wait(for: "xé\\\nb", in: app)
        // Command-B: bold, on the word at the cursor.
        app.typeKey("b", modifierFlags: .command)
        wait(for: "xé\\\n**b**", in: app)
    }

    /// Comment on Selection, write the comment, post it: its card shows it.
    @MainActor
    func testCommentingOnASelection() {
        let app = launch()
        app.typeText("First line.")
        app.typeKey(.upArrow, modifierFlags: .command)
        app.typeKey(.rightArrow, modifierFlags: [.option, .shift])
        app.typeKey("m", modifierFlags: [.command, .option])
        app.typeText("Why is this here?")
        app.typeKey(.enter, modifierFlags: .command)
        let comment = window(app).descendants(matching: .any)
            .matching(NSPredicate(format: "value == %@ OR label == %@", "Why is this here?", "Why is this here?"))
            .firstMatch
        XCTAssertTrue(comment.waitForExistence(timeout: 5), window(app).debugDescription)
    }
}
