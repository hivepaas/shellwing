import SwiftUI

/// A note's Markdown shown: headings, paragraphs, lists and tasks, quotes,
/// rules, code blocks and images (`attachment:<id>` from the vault). In a
/// session (`insert`), a code block's commands go into its shell.
struct MarkdownView: View {
    @EnvironmentObject private var app: AppModel
    let text: String
    let insert: ((String) -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(MarkdownBlock.parse(text).enumerated()), id: \.offset) { _, block in
                view(of: block)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .textSelection(.enabled)
    }

    @ViewBuilder
    private func view(of block: MarkdownBlock) -> some View {
        switch block {
        case .heading(let level, let text):
            inline(text)
                .font(.system(size: level == 1 ? 22 : level == 2 ? 19 : 16, weight: .semibold))
                .padding(.top, 4)
        case .paragraph(let text):
            inline(text).font(.system(size: 15))
        case .item(let marker, let text, let depth):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                switch marker {
                case .bullet: Text("•").foregroundStyle(Dusk.muted)
                case .number(let n): Text("\(n).").foregroundStyle(Dusk.muted).monospacedDigit()
                case .task(let done):
                    Image(systemName: done ? "checkmark.square.fill" : "square")
                        .foregroundStyle(done ? Dusk.accent : Dusk.muted)
                }
                inline(text).font(.system(size: 15))
            }
            .padding(.leading, CGFloat(depth) * 16)
        case .quote(let text):
            inline(text)
                .font(.system(size: 15))
                .foregroundStyle(Dusk.soft)
                .padding(.leading, 12)
                .overlay(alignment: .leading) { Dusk.edge.frame(width: 3) }
        case .rule:
            Dusk.line.frame(height: 1)
        case .code(let lang, let code):
            VStack(alignment: .leading, spacing: 0) {
                if insert != nil || !lang.isEmpty {
                    HStack {
                        Text(lang).font(.system(size: 11)).foregroundStyle(Dusk.muted)
                        Spacer()
                        if let insert {
                            Button("Insert", systemImage: "arrow.down.to.line") { insert(code) }
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(Dusk.accent)
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.top, 6)
                }
                ScrollView(.horizontal, showsIndicators: false) {
                    Text(code)
                        .font(.system(size: 13, design: .monospaced))
                        .foregroundStyle(Dusk.text)
                        .padding(10)
                }
            }
            .background(Dusk.field, in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(Dusk.line))
        case .image(let source, let alt):
            if let id = source.attachmentId, let image = app.noteImage(id) {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: 420, alignment: .leading)
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                    .accessibilityLabel(alt.isEmpty ? "Image" : alt)
            } else {
                Label(alt.isEmpty ? "The image cannot be shown" : alt, systemImage: "photo")
                    .font(.system(size: 13))
                    .foregroundStyle(Dusk.muted)
            }
        }
    }

    /// Bold, italics, code and links within a line.
    private func inline(_ text: String) -> Text {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        if let a = try? AttributedString(markdown: text, options: options) {
            return Text(a)
        }
        return Text(text)
    }
}

/// A block of Markdown, as the note view draws it.
enum MarkdownBlock {
    enum Marker { case bullet, number(Int), task(Bool) }

    case heading(Int, String)
    case paragraph(String)
    case item(Marker, String, depth: Int)
    case quote(String)
    case rule
    case code(lang: String, String)
    case image(source: String, alt: String)

    static func parse(_ text: String) -> [MarkdownBlock] {
        var out: [MarkdownBlock] = []
        var para: [String] = []
        var code: (fence: String, lang: String, lines: [String])?
        func flush() {
            if !para.isEmpty { out.append(.paragraph(para.joined(separator: "\n"))) }
            para = []
        }
        for raw in text.components(separatedBy: "\n") {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if let c = code {
                if line.hasPrefix(c.fence) {
                    out.append(.code(lang: c.lang, c.lines.joined(separator: "\n")))
                    code = nil
                } else {
                    code?.lines.append(raw)
                }
                continue
            }
            if line.hasPrefix("```") || line.hasPrefix("~~~") {
                flush()
                let fence = String(line.prefix(3))
                code = (fence, String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces), [])
                continue
            }
            if line.isEmpty { flush(); continue }
            if let h = heading(line) { flush(); out.append(h); continue }
            if ["---", "***", "___"].contains(line) { flush(); out.append(.rule); continue }
            if let img = image(line) { flush(); out.append(img); continue }
            if line.hasPrefix(">") {
                flush()
                out.append(.quote(String(line.dropFirst()).trimmingCharacters(in: .whitespaces)))
                continue
            }
            if let item = item(raw) { flush(); out.append(item); continue }
            para.append(line)
        }
        if let c = code { out.append(.code(lang: c.lang, c.lines.joined(separator: "\n"))) }
        flush()
        return out
    }

    private static func heading(_ line: String) -> MarkdownBlock? {
        let hashes = line.prefix { $0 == "#" }.count
        guard (1...6).contains(hashes), line.dropFirst(hashes).first == " " else { return nil }
        return .heading(hashes, String(line.dropFirst(hashes + 1)))
    }

    /// `![alt](source)` alone on its line.
    private static func image(_ line: String) -> MarkdownBlock? {
        guard line.hasPrefix("!["), line.hasSuffix(")"),
              let close = line.range(of: "](") else { return nil }
        let alt = String(line[line.index(line.startIndex, offsetBy: 2)..<close.lowerBound])
        let source = String(line[close.upperBound..<line.index(before: line.endIndex)])
        return .image(source: source, alt: alt)
    }

    private static func item(_ raw: String) -> MarkdownBlock? {
        let indent = raw.prefix { $0 == " " || $0 == "\t" }.count
        let line = raw.dropFirst(indent)
        let depth = min(indent / 2, 4)
        if let first = line.first, "-*+".contains(first), line.dropFirst().first == " " {
            let rest = String(line.dropFirst(2))
            for (box, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] where rest.hasPrefix(box) {
                return .item(.task(done), String(rest.dropFirst(4)), depth: depth)
            }
            return .item(.bullet, rest, depth: depth)
        }
        let digits = line.prefix { $0.isNumber }
        if !digits.isEmpty, let n = Int(digits), line.dropFirst(digits.count).hasPrefix(". ") {
            return .item(.number(n), String(line.dropFirst(digits.count + 2)), depth: depth)
        }
        return nil
    }
}

private extension String {
    /// The id of an `attachment:<id>` source.
    var attachmentId: String? {
        hasPrefix("attachment:") ? String(dropFirst("attachment:".count)) : nil
    }
}
