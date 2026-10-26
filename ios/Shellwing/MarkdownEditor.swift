import SwiftUI
import UIKit

/// What drives a note's editor from outside it: the bar's edits, an image
/// put in where the caret is. The edits themselves are the core's
/// (`md_edit.rs`), as the desktop's editor makes them; this applies them to
/// the text view as the keyboard would, so Undo takes them back.
@MainActor
final class MarkdownEditorController: ObservableObject {
    weak var textView: UITextView?
    /// The bar's image button: the note's editor picks one.
    var onImage: (() -> Void)?
    /// The bar's table button: the note's editor opens the grid, on the
    /// table the caret is in, or on a new one (nil).
    var onTable: ((MdTable?) -> Void)?
    /// The image an image's line shows (`attachment:<id>`), if there is one.
    /// Set: the images shown again.
    var imageFor: ((String) -> UIImage?)? {
        didSet {
            if let tv = textView, let c = tv.delegate as? MarkdownEditor.Coordinator { c.restyle(tv) }
        }
    }

    private var text: String { textView?.text ?? "" }

    private var selection: (UInt32, UInt32) {
        let r = textView?.selectedRange ?? NSRange(location: 0, length: 0)
        return (UInt32(r.location), UInt32(r.location + r.length))
    }

    func wrap(_ w: MdWrap) {
        let (s, e) = selection
        apply(mdToggleWrap(text: text, start: s, end: e, wrap: w))
    }

    func lines(_ kind: MdLines) {
        let (s, e) = selection
        apply(mdToggleLines(text: text, start: s, end: e, kind: kind))
    }

    func codeBlock() {
        let (s, e) = selection
        apply(mdCodeBlock(text: text, start: s, end: e))
    }

    func link() {
        let (s, e) = selection
        apply(mdLink(text: text, start: s, end: e))
    }

    func indent(out: Bool) {
        let (s, e) = selection
        apply(mdIndent(text: text, start: s, end: e, outdent: out))
    }

    /// `block` (an image's line) on its own line where the caret is.
    func insertBlock(_ block: String) {
        let (s, e) = selection
        apply(mdInsertBlock(text: text, start: s, end: e, block: block))
    }

    /// The grid on the table the caret is in, or on a new one.
    func table() {
        let (s, _) = selection
        onTable?(mdTableAt(text: text, at: s))
    }

    /// The table `original` (nil: a new one, put in where the caret is) made
    /// as the grid has it; with no columns, gone.
    func setTable(_ original: MdTable?, aligns: [MdAlign], head: [String], rows: [[String]]) {
        let table = head.isEmpty ? "" : mdTableText(aligns: aligns, head: head, rows: rows)
        if let t = original {
            apply(mdReplace(text: text, start: t.start, len: t.len, with: table))
        } else if !table.isEmpty {
            let (s, e) = selection
            apply(mdInsertBlock(text: text, start: s, end: e, block: table))
        }
    }

    /// Ticks or unticks the task whose box (`[ ]`) starts at `box`, as
    /// typed (Undo unticks).
    func toggleTask(box: Int) {
        guard let tv = textView,
              let a = tv.position(from: tv.beginningOfDocument, offset: box + 1),
              let b = tv.position(from: a, offset: 1),
              let middle = tv.textRange(from: a, to: b) else { return }
        let mark = tv.text(in: middle)
        guard mark == " " || mark == "x" || mark == "X" else { return }
        tv.replace(middle, withText: mark == " " ? "x" : " ")
    }

    func undo() { textView?.undoManager?.undo() }
    func redo() { textView?.undoManager?.redo() }

    /// Makes the text `edit.text` by replacing only what changed, through
    /// the text view's own input (so Undo has it), then selects as it says.
    func apply(_ edit: MdEdit) {
        guard let tv = textView else { return }
        let old = (tv.text ?? "") as NSString
        let new = edit.text as NSString
        var p = 0
        let most = min(old.length, new.length)
        while p < most && old.character(at: p) == new.character(at: p) { p += 1 }
        var q = 0
        while q < most - p && old.character(at: old.length - 1 - q) == new.character(at: new.length - 1 - q) { q += 1 }
        // Never half of a character: back off a surrogate pair's first half.
        if p > 0 && UTF16.isLeadSurrogate(old.character(at: p - 1)) { p -= 1 }
        if q > 0 && UTF16.isTrailSurrogate(old.character(at: old.length - q)) { q -= 1 }
        let gone = NSRange(location: p, length: old.length - q - p)
        let put = new.substring(with: NSRange(location: p, length: new.length - q - p))
        if let a = tv.position(from: tv.beginningOfDocument, offset: gone.location),
           let b = tv.position(from: a, offset: gone.length),
           let range = tv.textRange(from: a, to: b) {
            tv.replace(range, withText: put)
        }
        let start = Int(edit.selStart)
        let end = max(start, Int(edit.selEnd))
        tv.selectedRange = NSRange(location: start, length: end - start)
    }
}

/// A Markdown note's text, edited as Markdown and styled as it is typed:
/// marks dimmed, headings large, code in a monospace face, links in color,
/// a task's box ticked by a tap. Off the caret's line, as on the desktop,
/// the marks of formatting, headings and links hide (made invisible, not
/// taken away: the text and its offsets stay as they are) and an image's
/// line shows the image. A bar over the keyboard formats; Return carries a
/// list on.
struct MarkdownEditor: UIViewRepresentable {
    @Binding var text: String
    let controller: MarkdownEditorController

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> UITextView {
        let tv = UITextView(usingTextLayoutManager: true)
        tv.backgroundColor = .clear
        tv.font = Coordinator.body
        tv.textColor = UIColor(hex: 0xe7ebf0)
        tv.tintColor = UIColor(hex: 0x4cc4f9)
        tv.keyboardAppearance = .dark
        tv.keyboardDismissMode = .interactive
        tv.alwaysBounceVertical = true
        tv.textContainerInset = UIEdgeInsets(top: 12, left: 12, bottom: 24, right: 12)
        // A note holds commands: nothing changes what is typed.
        tv.autocorrectionType = .no
        tv.smartQuotesType = .no
        tv.smartDashesType = .no
        tv.smartInsertDeleteType = .no
        tv.spellCheckingType = .yes
        tv.delegate = context.coordinator
        tv.textLayoutManager?.delegate = context.coordinator
        tv.text = text
        tv.inputAccessoryView = context.coordinator.bar()
        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.tapped(_:)))
        tap.delegate = context.coordinator
        tv.addGestureRecognizer(tap)
        controller.textView = tv
        context.coordinator.restyle(tv)
        return tv
    }

    func updateUIView(_ tv: UITextView, context: Context) {
        context.coordinator.parent = self
        controller.textView = tv
        // Set from outside (a note loaded, a task ticked elsewhere).
        if tv.text != text && tv.markedTextRange == nil {
            let sel = tv.selectedRange
            tv.text = text
            let n = (text as NSString).length
            tv.selectedRange = NSRange(location: min(sel.location, n), length: 0)
            context.coordinator.restyle(tv)
        }
    }

    final class Coordinator: NSObject, UITextViewDelegate, UIGestureRecognizerDelegate, NSTextLayoutManagerDelegate {
        var parent: MarkdownEditor
        /// The runs of the text as last styled: where the task boxes are.
        private var runs: [MdRun] = []
        /// Its lines that are an image alone.
        private var images: [MdImageLine] = []
        /// The images shown, by source, at the size they show.
        private var shots: [String: (image: UIImage, size: CGSize)] = [:]
        /// The lines the caret (or the selection) is on: their marks show.
        private var caret = NSRange(location: 0, length: 0)
        /// Styling, which moves no caret of its own.
        private var painting = false
        private var pending: DispatchWorkItem?

        static let size: CGFloat = 16
        static let body = UIFont.systemFont(ofSize: size)
        static let base: [NSAttributedString.Key: Any] = {
            let p = NSMutableParagraphStyle()
            p.lineSpacing = 3
            return [.font: body, .foregroundColor: UIColor(hex: 0xe7ebf0), .paragraphStyle: p]
        }()

        init(_ parent: MarkdownEditor) { self.parent = parent }

        // MARK: Typing

        func textViewDidChange(_ tv: UITextView) {
            parent.text = tv.text
            // While a word is being composed (Telex, VNI, kana), its marked
            // text is the input method's: styled once it is in.
            guard tv.markedTextRange == nil else { return }
            // A long note is styled once typing pauses.
            if (tv.text as NSString).length > 30_000 {
                pending?.cancel()
                let work = DispatchWorkItem { [weak self, weak tv] in
                    if let tv, tv.markedTextRange == nil { self?.restyle(tv) }
                }
                pending = work
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.25, execute: work)
            } else {
                restyle(tv)
            }
        }

        func textView(_ tv: UITextView, shouldChangeTextIn range: NSRange, replacementText t: String) -> Bool {
            // Return on a list item or in a quote carries it on.
            guard t == "\n", range.length == 0, tv.markedTextRange == nil,
                  let edit = mdNewline(text: tv.text, at: UInt32(range.location)) else { return true }
            MainActor.assumeIsolated { parent.controller.apply(edit) }
            return false
        }

        // MARK: Style

        /// The text read again and styled whole.
        func restyle(_ tv: UITextView) {
            let text = tv.text ?? ""
            runs = markdownRuns(text: text)
            images = markdownImages(text: text)
            caret = caretLines(tv)
            paint(tv, NSRange(location: 0, length: tv.textStorage.length))
        }

        /// The caret on another line: the lines it left and the ones it is on
        /// styled again (their marks shown or hidden, an image or its line).
        func textViewDidChangeSelection(_ tv: UITextView) {
            guard !painting, tv.markedTextRange == nil else { return }
            let now = caretLines(tv)
            guard now != caret else { return }
            let old = caret
            caret = now
            let n = tv.textStorage.length
            let a = NSIntersectionRange(old, NSRange(location: 0, length: n))
            paint(tv, a.length > 0 ? NSUnionRange(a, now) : now)
        }

        private func caretLines(_ tv: UITextView) -> NSRange {
            let ns = (tv.text ?? "") as NSString
            let sel = tv.selectedRange
            guard sel.location <= ns.length else { return NSRange(location: ns.length, length: 0) }
            return ns.lineRange(for: sel)
        }

        private func onCaret(_ r: NSRange) -> Bool {
            NSIntersectionRange(r, caret).length > 0
                || (r.length == 0 && NSLocationInRange(r.location, caret))
                || (caret.length == 0 && caret.location >= r.location && caret.location <= NSMaxRange(r))
        }

        /// Styles `range` of the text from the runs read.
        private func paint(_ tv: UITextView, _ range: NSRange) {
            let storage = tv.textStorage
            let range = NSIntersectionRange(range, NSRange(location: 0, length: storage.length))
            painting = true
            defer { painting = false }
            let sel = tv.selectedRange
            // An image not here (not synced yet): its line shows as written.
            let missing = images.compactMap { img -> NSRange? in
                let line = NSRange(location: Int(img.start), length: Int(img.len))
                return shot(img.source, tv) == nil ? line : nil
            }
            storage.beginEditing()
            storage.setAttributes(Self.base, range: range)
            for r in runs {
                let rr = NSRange(location: Int(r.start), length: Int(r.len))
                let part = NSIntersectionRange(rr, range)
                guard part.length > 0, NSMaxRange(part) <= storage.length else { continue }
                let hidden = r.hide && !onCaret(rr) && !missing.contains { NSIntersectionRange($0, rr).length > 0 }
                storage.addAttributes(hidden ? Self.hidden : Self.attributes(r), range: part)
            }
            for img in images {
                let line = NSRange(location: Int(img.start), length: Int(img.len))
                guard NSIntersectionRange(line, range).length > 0, NSMaxRange(line) <= storage.length,
                      !onCaret(line), let shot = shot(img.source, tv) else { continue }
                // The line's text hidden, its height the image's: the image
                // is drawn there (`ImageFragment`).
                let p = NSMutableParagraphStyle()
                p.minimumLineHeight = shot.size.height + 8
                p.maximumLineHeight = shot.size.height + 8
                var a = Self.hidden
                a[.paragraphStyle] = p
                storage.addAttributes(a, range: line)
            }
            storage.endEditing()
            if tv.selectedRange != sel { tv.selectedRange = sel }
            tv.typingAttributes = Self.base
        }

        /// The image `source` names, at the width the text has.
        private func shot(_ source: String, _ tv: UITextView) -> (image: UIImage, size: CGSize)? {
            let width = max(tv.bounds.width, 300) - tv.textContainerInset.left - tv.textContainerInset.right - 12
            if let s = shots[source], abs(s.size.width - min(width, s.image.size.width)) < 1 || s.size.height >= 320 {
                return s
            }
            guard let image = MainActor.assumeIsolated({ parent.controller.imageFor?(source) }),
                  image.size.width > 0 else { return nil }
            var size = CGSize(width: min(width, image.size.width), height: 0)
            size.height = size.width * image.size.height / image.size.width
            if size.height > 320 {
                size = CGSize(width: 320 * image.size.width / image.size.height, height: 320)
            }
            shots[source] = (image, size)
            return (image, size)
        }

        // MARK: Images (TextKit 2)

        func textLayoutManager(
            _ m: NSTextLayoutManager,
            textLayoutFragmentFor location: any NSTextLocation,
            in textElement: NSTextElement
        ) -> NSTextLayoutFragment {
            let range = textElement.elementRange
            if let range, let content = m.textContentManager {
                let start = content.offset(from: content.documentRange.location, to: range.location)
                if let img = images.first(where: { Int($0.start) == start }),
                   !onCaret(NSRange(location: Int(img.start), length: Int(img.len))),
                   let shot = shots[img.source] {
                    let f = ImageFragment(textElement: textElement, range: range)
                    f.image = shot.image
                    f.size = shot.size
                    return f
                }
            }
            return NSTextLayoutFragment(textElement: textElement, range: range)
        }

        /// What hides: no size, no color, the text still there.
        static let hidden: [NSAttributedString.Key: Any] = [
            .font: UIFont.systemFont(ofSize: 0.01),
            .foregroundColor: UIColor.clear,
        ]

        static func attributes(_ r: MdRun) -> [NSAttributedString.Key: Any] {
            let scale: CGFloat = [1, 1.8, 1.5, 1.3, 1.15, 1.05, 1][Int(min(r.heading, 6))]
            let size = size * scale
            let weight: UIFont.Weight = r.heading > 0 ? .semibold : (r.bold ? .bold : .regular)
            var font = r.mono
                ? UIFont.monospacedSystemFont(ofSize: size * 0.94, weight: weight)
                : UIFont.systemFont(ofSize: size, weight: weight)
            if r.italic, let d = font.fontDescriptor.withSymbolicTraits(font.fontDescriptor.symbolicTraits.union(.traitItalic)) {
                font = UIFont(descriptor: d, size: font.pointSize)
            }
            var a: [NSAttributedString.Key: Any] = [.font: font]
            if r.strike {
                a[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
                a[.strikethroughColor] = UIColor(hex: 0xa0aab7)
            }
            switch r.role {
            case .plain: break
            case .marker, .html: a[.foregroundColor] = UIColor(hex: 0x7b8595)
            case .code:
                a[.foregroundColor] = UIColor(hex: 0xe7ebf0)
                a[.backgroundColor] = UIColor(hex: 0x1c2129)
            case .codeBlock: a[.backgroundColor] = UIColor(hex: 0x1c2129)
            case .link: a[.foregroundColor] = UIColor(hex: 0x4cc4f9)
            case .quote: a[.foregroundColor] = UIColor(hex: 0xc9d0d9)
            case .listMarker, .taskBox: a[.foregroundColor] = UIColor(hex: 0x4cc4f9)
            case .done: a[.foregroundColor] = UIColor(hex: 0xa0aab7)
            case .alert(let kind):
                a[.foregroundColor] = UIColor(kind.color)
                a[.font] = UIFont.systemFont(ofSize: size, weight: .semibold)
            }
            return a
        }

        // MARK: Tasks

        /// A tap on a task's box ticks it (Undo unticks).
        @objc func tapped(_ g: UITapGestureRecognizer) {
            guard let tv = g.view as? UITextView else { return }
            let point = g.location(in: tv)
            for r in runs where r.role == .taskBox && r.len >= 3 {
                guard let a = tv.position(from: tv.beginningOfDocument, offset: Int(r.start)),
                      let b = tv.position(from: a, offset: 3),
                      let range = tv.textRange(from: a, to: b) else { continue }
                let box = tv.firstRect(for: range).insetBy(dx: -8, dy: -6)
                guard box.contains(point) else { continue }
                MainActor.assumeIsolated { parent.controller.toggleTask(box: Int(r.start)) }
                return
            }
        }

        func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
            true
        }

        // MARK: The bar

        /// The bar over the keyboard: formats, lists, blocks, indent, an
        /// image, Undo and Redo, and a way to put the keyboard away.
        func bar() -> UIView {
            let bar = UIView(frame: CGRect(x: 0, y: 0, width: 320, height: 46))
            bar.autoresizingMask = .flexibleWidth
            bar.backgroundColor = UIColor(hex: 0x21272f)
            let line = UIView()
            line.backgroundColor = UIColor(hex: 0x363e4a)
            line.translatesAutoresizingMaskIntoConstraints = false
            bar.addSubview(line)

            let scroll = UIScrollView()
            scroll.showsHorizontalScrollIndicator = false
            scroll.translatesAutoresizingMaskIntoConstraints = false
            bar.addSubview(scroll)
            let stack = UIStackView()
            stack.axis = .horizontal
            stack.spacing = 2
            stack.translatesAutoresizingMaskIntoConstraints = false
            scroll.addSubview(stack)

            let c = parent.controller
            let items: [(String, String, () -> Void)] = [
                ("bold", tr("Bold"), { c.wrap(.bold) }),
                ("italic", tr("Italic"), { c.wrap(.italic) }),
                ("strikethrough", tr("Strikethrough"), { c.wrap(.strike) }),
                ("chevron.left.forwardslash.chevron.right", tr("Code"), { c.wrap(.code) }),
                ("link", tr("Link"), { c.link() }),
                ("number", tr("Heading"), { c.lines(.heading) }),
                ("list.bullet", tr("Bulleted list"), { c.lines(.bullet) }),
                ("list.number", tr("Numbered list"), { c.lines(.numbered) }),
                ("checklist", tr("To-do"), { c.lines(.task) }),
                ("text.quote", tr("Quote"), { c.lines(.quote) }),
                ("curlybraces", tr("Code block"), { c.codeBlock() }),
                ("tablecells", tr("Table"), { c.table() }),
                ("decrease.indent", tr("Outdent"), { c.indent(out: true) }),
                ("increase.indent", tr("Indent"), { c.indent(out: false) }),
                ("photo", tr("Insert an image…"), { c.onImage?() }),
                ("arrow.uturn.backward", tr("Undo"), { c.undo() }),
                ("arrow.uturn.forward", tr("Redo"), { c.redo() }),
            ]
            for (symbol, label, act) in items {
                stack.addArrangedSubview(button(symbol, label, act))
            }
            let hide = button("keyboard.chevron.compact.down", tr("Hide the keyboard")) { [weak c] in
                c?.textView?.resignFirstResponder()
            }
            hide.translatesAutoresizingMaskIntoConstraints = false
            bar.addSubview(hide)

            NSLayoutConstraint.activate([
                line.topAnchor.constraint(equalTo: bar.topAnchor),
                line.leadingAnchor.constraint(equalTo: bar.leadingAnchor),
                line.trailingAnchor.constraint(equalTo: bar.trailingAnchor),
                line.heightAnchor.constraint(equalToConstant: 1),
                hide.trailingAnchor.constraint(equalTo: bar.trailingAnchor, constant: -4),
                hide.centerYAnchor.constraint(equalTo: bar.centerYAnchor),
                scroll.leadingAnchor.constraint(equalTo: bar.leadingAnchor, constant: 4),
                scroll.trailingAnchor.constraint(equalTo: hide.leadingAnchor, constant: -4),
                scroll.topAnchor.constraint(equalTo: bar.topAnchor, constant: 1),
                scroll.bottomAnchor.constraint(equalTo: bar.bottomAnchor),
                stack.leadingAnchor.constraint(equalTo: scroll.contentLayoutGuide.leadingAnchor),
                stack.trailingAnchor.constraint(equalTo: scroll.contentLayoutGuide.trailingAnchor),
                stack.topAnchor.constraint(equalTo: scroll.contentLayoutGuide.topAnchor),
                stack.bottomAnchor.constraint(equalTo: scroll.contentLayoutGuide.bottomAnchor),
                stack.heightAnchor.constraint(equalTo: scroll.frameLayoutGuide.heightAnchor),
            ])
            return bar
        }

        private func button(_ symbol: String, _ label: String, _ act: @escaping () -> Void) -> UIButton {
            var config = UIButton.Configuration.plain()
            config.image = UIImage(systemName: symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: 16, weight: .medium))
            config.baseForegroundColor = UIColor(hex: 0xe7ebf0)
            config.contentInsets = NSDirectionalEdgeInsets(top: 8, leading: 9, bottom: 8, trailing: 9)
            let b = UIButton(configuration: config, primaryAction: UIAction { _ in
                MainActor.assumeIsolated { act() }
            })
            b.accessibilityLabel = label
            b.widthAnchor.constraint(greaterThanOrEqualToConstant: 40).isActive = true
            return b
        }
    }
}

/// An image's line, off the caret's: the image drawn where its hidden text
/// is, the line as tall as the image.
final class ImageFragment: NSTextLayoutFragment {
    var image: UIImage?
    var size: CGSize = .zero

    override func draw(at point: CGPoint, in context: CGContext) {
        guard let image else {
            super.draw(at: point, in: context)
            return
        }
        let rect = CGRect(x: point.x + 6, y: point.y + 4, width: size.width, height: size.height)
        UIGraphicsPushContext(context)
        context.saveGState()
        UIBezierPath(roundedRect: rect, cornerRadius: 6).addClip()
        image.draw(in: rect)
        context.restoreGState()
        UIGraphicsPopContext()
    }

    override var renderingSurfaceBounds: CGRect {
        super.renderingSurfaceBounds.union(CGRect(x: 0, y: 0, width: size.width + 12, height: size.height + 8))
    }
}
