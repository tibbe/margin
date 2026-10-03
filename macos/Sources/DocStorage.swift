import AppKit

/// The document's text. Every change to its characters, whoever makes it
/// (the core's plans, undo, input methods), goes through
/// `replaceCharacters(in:with:)`, which reports it to `onReplace` as it is
/// made. The storage's own edit notifications don't serve for that: they
/// join the edits of a batch into one range, and fixing attributes can
/// widen it. Like the view it backs, it is only used on the main thread.
nonisolated final class DocStorage: NSTextStorage {
    private let backing = NSMutableAttributedString()

    /// An edit to the characters: the range replaced, in the text before
    /// it, and the length of what replaced it.
    var onReplace: (@MainActor (NSRange, Int) -> Void)?

    /// The text as of the last edit. Bridging the backing's mutable
    /// string would copy it on every read, and layout reads it often; an
    /// immutable copy bridges as it is.
    private var text: String?

    override var string: String {
        if let text { return text }
        let t = backing.mutableString.copy() as! NSString as String
        text = t
        return t
    }

    override func attributes(at location: Int, effectiveRange range: NSRangePointer?) -> [NSAttributedString.Key: Any] {
        backing.attributes(at: location, effectiveRange: range)
    }

    // Reads of one attribute go straight to the backing, rather than
    // through `attributes(at:effectiveRange:)`, which bridges every
    // attribute there on every call.

    override var length: Int { backing.length }

    override func attribute(
        _ name: NSAttributedString.Key, at location: Int, effectiveRange range: NSRangePointer?
    ) -> Any? {
        backing.attribute(name, at: location, effectiveRange: range)
    }

    override func attribute(
        _ name: NSAttributedString.Key, at location: Int, longestEffectiveRange range: NSRangePointer?,
        in rangeLimit: NSRange
    ) -> Any? {
        backing.attribute(name, at: location, longestEffectiveRange: range, in: rangeLimit)
    }

    override func enumerateAttribute(
        _ name: NSAttributedString.Key, in range: NSRange, options: NSAttributedString.EnumerationOptions = [],
        using block: (Any?, NSRange, UnsafeMutablePointer<ObjCBool>) -> Void
    ) {
        backing.enumerateAttribute(name, in: range, options: options, using: block)
    }

    override func enumerateAttributes(
        in range: NSRange, options: NSAttributedString.EnumerationOptions = [],
        using block: ([NSAttributedString.Key: Any], NSRange, UnsafeMutablePointer<ObjCBool>) -> Void
    ) {
        backing.enumerateAttributes(in: range, options: options, using: block)
    }

    override func attributes(
        at location: Int, longestEffectiveRange range: NSRangePointer?, in rangeLimit: NSRange
    ) -> [NSAttributedString.Key: Any] {
        backing.attributes(at: location, longestEffectiveRange: range, in: rangeLimit)
    }

    override func attributedSubstring(from range: NSRange) -> NSAttributedString {
        backing.attributedSubstring(from: range)
    }

    override func addAttribute(_ name: NSAttributedString.Key, value: Any, range: NSRange) {
        beginEditing()
        backing.addAttribute(name, value: value, range: range)
        edited(.editedAttributes, range: range, changeInLength: 0)
        endEditing()
    }

    override func replaceCharacters(in range: NSRange, with str: String) {
        let length = (str as NSString).length
        beginEditing()
        backing.replaceCharacters(in: range, with: str)
        text = nil
        edited(.editedCharacters, range: range, changeInLength: length - range.length)
        if let onReplace { MainActor.assumeIsolated { onReplace(range, length) } }
        endEditing()
    }

    override func setAttributes(_ attrs: [NSAttributedString.Key: Any]?, range: NSRange) {
        beginEditing()
        backing.setAttributes(attrs, range: range)
        edited(.editedAttributes, range: range, changeInLength: 0)
        endEditing()
    }
}
