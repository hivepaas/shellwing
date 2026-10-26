import SwiftUI
import UIKit

/// The terminal, drawn from the session's frames, taking the keyboard.
struct TerminalView: UIViewRepresentable {
    @ObservedObject var model: SessionModel
    /// Whether the keyboard is up; a tap on the terminal raises it.
    @Binding var typing: Bool

    func makeUIView(context: Context) -> TerminalCanvas {
        let view = TerminalCanvas()
        view.model = model
        view.onFocus = { focused in
            DispatchQueue.main.async {
                if typing != focused { typing = focused }
            }
        }
        return view
    }

    func updateUIView(_ view: TerminalCanvas, context: Context) {
        view.model = model
        view.frameData = model.frame
        if typing, !view.isFirstResponder {
            _ = view.becomeFirstResponder()
        } else if !typing, view.isFirstResponder {
            _ = view.resignFirstResponder()
        }
    }
}

/// Draws a frame cell by cell (runs of styled text, their backgrounds, the
/// cursor) and passes what is typed to the session.
final class TerminalCanvas: UIView, UIKeyInput {
    weak var model: SessionModel?
    var onFocus: ((Bool) -> Void)?

    var frameData: FrameData? {
        didSet {
            if frameData?.seq != oldValue?.seq { setNeedsDisplay() }
        }
    }

    /// The desktop's default size, readable on a phone.
    private let font = UIFont.monospacedSystemFont(ofSize: 12.5, weight: .regular)
    private let bold = UIFont.monospacedSystemFont(ofSize: 12.5, weight: .bold)
    private var cell = CGSize(width: 7.5, height: 16)
    private var size: (cols: UInt16, rows: UInt16) = (0, 0)
    /// Rows of the history scrolled but not yet sent, a pan's leftover.
    private var scrollRest: CGFloat = 0
    private let inset = UIEdgeInsets(top: 4, left: 6, bottom: 4, right: 6)

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = UIColor(hex: 0x272e38)
        isOpaque = true
        contentMode = .redraw
        let w = ("M" as NSString).size(withAttributes: [.font: font]).width
        cell = CGSize(width: w, height: ceil(font.lineHeight + 2))
        addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tapped)))
        addGestureRecognizer(UIPanGestureRecognizer(target: self, action: #selector(panned(_:))))
        isAccessibilityElement = true
        accessibilityLabel = "Terminal"
        accessibilityTraits = .allowsDirectInteraction
    }

    required init?(coder: NSCoder) { fatalError("not from a storyboard") }

    // MARK: Size

    override func layoutSubviews() {
        super.layoutSubviews()
        let usable = bounds.inset(by: inset)
        let cols = UInt16(max(10, Int(usable.width / cell.width)))
        let rows = UInt16(max(4, Int(usable.height / cell.height)))
        guard (cols, rows) != size else { return }
        size = (cols, rows)
        let scale = window?.screen.scale ?? UIScreen.main.scale
        model?.resize(
            cols: cols, rows: rows,
            cellW: UInt32(cell.width * scale), cellH: UInt32(cell.height * scale)
        )
    }

    // MARK: Drawing

    override func draw(_ rect: CGRect) {
        guard let ctx = UIGraphicsGetCurrentContext() else { return }
        let f = frameData
        let ground = UIColor(hex: f?.background ?? 0x272e38)
        ctx.setFillColor(ground.cgColor)
        ctx.fill(bounds)
        guard let f else { return }
        for (y, row) in f.rows.enumerated() {
            let top = inset.top + CGFloat(y) * cell.height
            for run in row.runs {
                let x = inset.left + CGFloat(run.col) * cell.width
                let width = CGFloat(run.cells) * cell.width
                if let bg = run.bg {
                    ctx.setFillColor(UIColor(hex: bg).cgColor)
                    ctx.fill(CGRect(x: x, y: top, width: width, height: cell.height))
                }
                if run.selected {
                    ctx.setFillColor(UIColor(hex: 0x4cc4f9).withAlphaComponent(0.35).cgColor)
                    ctx.fill(CGRect(x: x, y: top, width: width, height: cell.height))
                }
                if run.text.trimmingCharacters(in: .whitespaces).isEmpty { continue }
                var attrs: [NSAttributedString.Key: Any] = [
                    .font: run.bold ? bold : font,
                    .foregroundColor: UIColor(hex: run.fg),
                ]
                if run.underline { attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue }
                if run.italic { attrs[.obliqueness] = 0.18 }
                drawCells(run, at: CGPoint(x: x, y: top + 1), attrs: attrs)
            }
        }
        if let col = f.cursorCol, let row = f.cursorRow, !f.scrolledBack {
            let r = CGRect(
                x: inset.left + CGFloat(col) * cell.width,
                y: inset.top + CGFloat(row) * cell.height,
                width: cell.width, height: cell.height
            )
            let color = UIColor(hex: f.cursorColor)
            if isFirstResponder {
                ctx.setFillColor(color.withAlphaComponent(0.85).cgColor)
                ctx.fill(r)
            } else {
                ctx.setStrokeColor(color.cgColor)
                ctx.setLineWidth(1)
                ctx.stroke(r.insetBy(dx: 0.5, dy: 0.5))
            }
        }
    }

    /// A run's text, one grapheme per cell so a font's fallback (CJK, emoji)
    /// cannot push the columns after it out of line.
    private func drawCells(_ run: RunData, at origin: CGPoint, attrs: [NSAttributedString.Key: Any]) {
        if run.wide {
            (run.text as NSString).draw(at: origin, withAttributes: attrs)
            return
        }
        if run.text.unicodeScalars.allSatisfy({ $0.isASCII }) {
            (run.text as NSString).draw(at: origin, withAttributes: attrs)
            return
        }
        var x = origin.x
        for g in run.text {
            (String(g) as NSString).draw(at: CGPoint(x: x, y: origin.y), withAttributes: attrs)
            x += cell.width
        }
    }

    // MARK: Gestures

    @objc private func tapped() {
        if isFirstResponder {
            _ = resignFirstResponder()
        } else {
            _ = becomeFirstResponder()
        }
    }

    @objc private func panned(_ pan: UIPanGestureRecognizer) {
        let dy = pan.translation(in: self).y
        pan.setTranslation(.zero, in: self)
        scrollRest += dy / cell.height
        let lines = scrollRest.rounded(.towardZero)
        if lines != 0 {
            scrollRest -= lines
            model?.scroll(lines: Float(lines))
        }
        if pan.state == .ended { scrollRest = 0 }
    }

    // MARK: The keyboard

    override var canBecomeFirstResponder: Bool { true }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok { onFocus?(true); setNeedsDisplay() }
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { onFocus?(false); setNeedsDisplay() }
        return ok
    }

    var hasText: Bool { true }

    func insertText(_ text: String) {
        Task { @MainActor in self.model?.type(text) }
    }

    func deleteBackward() {
        Task { @MainActor in self.model?.press(.backspace) }
    }

    // A terminal takes what is typed as it is: no corrections, no capitals,
    // no curly quotes.
    var autocorrectionType: UITextAutocorrectionType { get { .no } set {} }
    var autocapitalizationType: UITextAutocapitalizationType { get { .none } set {} }
    var spellCheckingType: UITextSpellCheckingType { get { .no } set {} }
    var smartQuotesType: UITextSmartQuotesType { get { .no } set {} }
    var smartDashesType: UITextSmartDashesType { get { .no } set {} }
    var smartInsertDeleteType: UITextSmartInsertDeleteType { get { .no } set {} }
    var keyboardAppearance: UIKeyboardAppearance { get { .dark } set {} }

    // Paste, from the edit menu a long press brings.
    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        action == #selector(paste(_:)) && UIPasteboard.general.hasStrings
    }

    override func paste(_ sender: Any?) {
        guard let text = UIPasteboard.general.string else { return }
        Task { @MainActor in self.model?.paste(text) }
    }
}
