import SwiftUI

/// A note's Markdown shown, as the desktop shows it: headings, paragraphs,
/// lists and tasks, quotes and alerts, code, tables, images
/// (`attachment:<id>` from the vault), rules. The core reads the Markdown
/// (`markdown_blocks`); this draws its blocks. A task's box ticks when
/// `toggle` is given (it gets the box's place in the text); in a session,
/// `insert` puts a code block's commands into its shell.
struct MarkdownView: View {
    @EnvironmentObject private var app: AppModel
    let text: String
    var insert: ((String) -> Void)?
    var toggle: ((UInt64) -> Void)?

    /// The body text's size: headings scale from it as the desktop's do.
    private let base: CGFloat = 15

    var body: some View {
        let blocks = markdownBlocks(text: text)
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { n, block in
                view(of: block)
                    .padding(.leading, CGFloat(block.indent) * 18)
                    .modifier(QuoteBars(depth: Int(block.quote), alert: block.alert))
                    .padding(.top, n == 0 ? 0 : gap(blocks[n - 1], block))
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// The space between two blocks: none inside a quote, whose bar and
    /// tint run on; little between items of a list.
    private func gap(_ a: MdBlock, _ b: MdBlock) -> CGFloat {
        if a.group != 0 && a.group == b.group { return 0 }
        if case .item = a.kind, case .item = b.kind { return 6 }
        return 10
    }

    @ViewBuilder
    private func view(of block: MdBlock) -> some View {
        switch block.kind {
        case .heading(let level, let spans):
            styled(spans)
                .font(.system(size: base * scale(level), weight: .semibold))
                .padding(.top, level <= 2 ? 6 : 2)
                .textSelection(.enabled)
        case .paragraph(let spans):
            styled(spans).font(.system(size: base)).textSelection(.enabled)
        case .item(let marker, let task, let at, let spans):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                if let done = task {
                    Button {
                        if let at { toggle?(at) }
                    } label: {
                        Image(systemName: done ? "checkmark.square.fill" : "square")
                            .foregroundStyle(done ? Dusk.accent : Dusk.muted)
                    }
                    .buttonStyle(.plain)
                    .disabled(toggle == nil || at == nil)
                    .accessibilityLabel(done ? tr("Done") : tr("Not done"))
                } else {
                    Text(marker).foregroundStyle(Dusk.muted).monospacedDigit()
                }
                styled(spans)
                    .font(.system(size: base))
                    .strikethrough(task == true, color: Dusk.muted)
                    .foregroundStyle(task == true ? Dusk.muted : Dusk.text)
                    .textSelection(.enabled)
            }
        case .code(let lang, let code):
            CodeBlock(lang: lang, code: code, insert: insert)
        case .table(let aligns, let head, let rows):
            table(aligns, head, rows)
        case .image(let source, let alt):
            image(source, alt)
        case .rule:
            Dusk.line.frame(height: 1).padding(.vertical, 4)
        case .alertTitle(let kind):
            Label(kind.title, systemImage: kind.symbol)
                .font(.system(size: base, weight: .semibold))
                .foregroundStyle(kind.color)
        }
    }

    private func scale(_ level: UInt8) -> CGFloat {
        switch level {
        case 1: return 1.8
        case 2: return 1.5
        case 3: return 1.3
        case 4: return 1.15
        case 5: return 1.05
        default: return 1
        }
    }

    /// Spans as one text: bold, italic, struck, code, links that open.
    private func styled(_ spans: [MdSpan]) -> Text {
        var out = AttributedString()
        for s in spans {
            var a = AttributedString(s.text)
            var intent: InlinePresentationIntent = []
            if s.bold { intent.insert(.stronglyEmphasized) }
            if s.italic { intent.insert(.emphasized) }
            if s.strike { intent.insert(.strikethrough) }
            if s.code {
                intent.insert(.code)
                a.font = .system(size: base - 1, design: .monospaced)
                a.backgroundColor = Dusk.field
            }
            a.inlinePresentationIntent = intent
            if let link = s.link, let url = URL(string: link) {
                a.link = url
                a.foregroundColor = Dusk.accent
                a.underlineStyle = .single
            }
            out += a
        }
        return Text(out)
    }

    private func table(_ aligns: [MdAlign], _ head: [MdCell], _ rows: [MdRow]) -> some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(Array(head.enumerated()), id: \.offset) { n, cell in
                        cellView(cell, align: n < aligns.count ? aligns[n] : .none)
                            .font(.system(size: base - 1, weight: .semibold))
                            .background(Dusk.raised)
                    }
                }
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    GridRow {
                        ForEach(Array(row.cells.enumerated()), id: \.offset) { n, cell in
                            cellView(cell, align: n < aligns.count ? aligns[n] : .none)
                                .font(.system(size: base - 1))
                        }
                    }
                }
            }
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(Dusk.line))
            .clipShape(RoundedRectangle(cornerRadius: 6))
        }
    }

    private func cellView(_ cell: MdCell, align: MdAlign) -> some View {
        styled(cell.spans)
            .textSelection(.enabled)
            .multilineTextAlignment(align == .right ? .trailing : align == .center ? .center : .leading)
            .frame(minWidth: 60, maxWidth: 280, alignment: align == .right ? .trailing : align == .center ? .center : .leading)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .overlay(alignment: .trailing) { Dusk.line.frame(width: 1) }
            .overlay(alignment: .bottom) { Dusk.line.frame(height: 1) }
    }

    @ViewBuilder
    private func image(_ source: String, _ alt: String) -> some View {
        let id = source.hasPrefix("attachment:") ? String(source.dropFirst("attachment:".count)) : nil
        if let id, let image = app.noteImage(id) {
            Image(uiImage: image)
                .resizable()
                .scaledToFit()
                .frame(maxWidth: .infinity, maxHeight: 420, alignment: .leading)
                .clipShape(RoundedRectangle(cornerRadius: 6))
                .accessibilityLabel(alt.isEmpty ? tr("Image") : alt)
        } else {
            Label(alt.isEmpty ? tr("The image cannot be shown") : alt, systemImage: "photo")
                .font(.system(size: 13))
                .foregroundStyle(Dusk.muted)
        }
    }
}

/// A code block: its language, Copy, and Insert in a session.
private struct CodeBlock: View {
    let lang: String
    let code: String
    let insert: ((String) -> Void)?
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 14) {
                Text(lang).font(.system(size: 11)).foregroundStyle(Dusk.muted)
                Spacer()
                Button(copied ? tr("Copied") : tr("Copy"), systemImage: copied ? "checkmark" : "doc.on.doc") {
                    UIPasteboard.general.string = code
                    copied = true
                    Task {
                        try? await Task.sleep(for: .seconds(2))
                        copied = false
                    }
                }
                if let insert {
                    Button(tr("Insert"), systemImage: "arrow.down.to.line") { insert(code) }
                }
            }
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(Dusk.accent)
            .buttonStyle(.plain)
            .padding(.horizontal, 10)
            .padding(.top, 7)
            ScrollView(.horizontal, showsIndicators: false) {
                Text(code)
                    .font(.system(size: 13, design: .monospaced))
                    .foregroundStyle(Dusk.text)
                    .textSelection(.enabled)
                    .padding(10)
            }
        }
        .background(Dusk.field, in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Dusk.line))
    }
}

/// The bars of the quotes a block is in; an alert's in its color, its
/// background tinted.
private struct QuoteBars: ViewModifier {
    let depth: Int
    let alert: MdAlert?

    func body(content: Content) -> some View {
        if depth == 0 {
            content
        } else {
            HStack(alignment: .top, spacing: 0) {
                ForEach(0..<depth, id: \.self) { n in
                    (n == depth - 1 ? (alert?.color ?? Dusk.edge) : Dusk.edge)
                        .frame(width: 3)
                        .padding(.trailing, 10)
                }
                // The space inside, so the bars run the block's whole height
                // and meet the next block's.
                content
                    .foregroundStyle(alert == nil ? Dusk.soft : Dusk.text)
                    .padding(.vertical, 5)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .fixedSize(horizontal: false, vertical: true)
            .background(alert.map { $0.color.opacity(0.08) } ?? .clear)
        }
    }
}

extension MdAlert {
    var title: String {
        switch self {
        case .note: return tr("Note")
        case .tip: return tr("Tip")
        case .important: return tr("Important")
        case .warning: return tr("Warning")
        case .caution: return tr("Caution")
        }
    }

    var symbol: String {
        switch self {
        case .note: return "info.circle"
        case .tip: return "lightbulb"
        case .important: return "exclamationmark.bubble"
        case .warning: return "exclamationmark.triangle"
        case .caution: return "exclamationmark.octagon"
        }
    }

    var color: Color {
        switch self {
        case .note: return Dusk.accent
        case .tip: return Dusk.ok
        case .important: return Color(hex: 0xa78bfa)
        case .warning: return Dusk.amber
        case .caution: return Dusk.bad
        }
    }
}
