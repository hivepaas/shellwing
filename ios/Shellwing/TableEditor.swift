import SwiftUI

/// A note's table as a grid, as the desktop's table menus edit it: cells
/// typed in, rows and columns put in, duplicated and deleted, a column's
/// alignment chosen. Saved, the core writes it back as Markdown, its
/// columns lined up (`md_table_text`).
struct TableEditor: View {
    @Environment(\.dismiss) private var dismiss
    /// The table edited; nil: a new one.
    let original: MdTable?
    let save: (_ aligns: [MdAlign], _ head: [String], _ rows: [[String]]) -> Void

    struct Column: Identifiable {
        let id = UUID()
        var align: MdAlign
    }

    struct Row: Identifiable {
        let id = UUID()
        var cells: [String]
    }

    @State private var columns: [Column] = []
    @State private var head: [String] = []
    @State private var rows: [Row] = []
    @State private var loaded = false
    @State private var confirmDelete = false

    var body: some View {
        NavigationStack {
            ScrollView([.horizontal, .vertical]) {
                Grid(alignment: .leading, horizontalSpacing: 6, verticalSpacing: 6) {
                    GridRow {
                        Color.clear.frame(width: 30, height: 1)
                        ForEach(Array(columns.enumerated()), id: \.element.id) { i, col in
                            columnMenu(i, col)
                        }
                    }
                    GridRow {
                        Color.clear.frame(width: 30, height: 1)
                        ForEach(Array(columns.enumerated()), id: \.element.id) { i, col in
                            cell(Binding(get: { head[i] }, set: { head[i] = $0 }), align: col.align, bold: true)
                        }
                    }
                    ForEach(Array(rows.enumerated()), id: \.element.id) { r, row in
                        GridRow {
                            rowMenu(r)
                            ForEach(Array(columns.enumerated()), id: \.element.id) { i, col in
                                cell(Binding(get: { rows[r].cells[i] }, set: { rows[r].cells[i] = $0 }), align: col.align, bold: false)
                            }
                        }
                    }
                }
                .padding(16)
            }
            .background(Dusk.page.ignoresSafeArea())
            .safeAreaInset(edge: .bottom) {
                HStack(spacing: 10) {
                    Button(tr("Insert row below"), systemImage: "plus") { insertRow(at: rows.count) }
                    Button(tr("Insert column right"), systemImage: "plus") { insertColumn(at: columns.count) }
                    Spacer()
                    if original != nil {
                        Button(tr("Delete table"), systemImage: "trash", role: .destructive) { confirmDelete = true }
                            .labelStyle(.iconOnly)
                            .confirmationDialog(tr("Delete table"), isPresented: $confirmDelete, titleVisibility: .visible) {
                                Button(tr("Delete table"), role: .destructive) {
                                    save([], [], [])
                                    dismiss()
                                }
                            }
                    }
                }
                .font(.system(size: 14, weight: .medium))
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                .buttonStyle(.bordered)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .background(Dusk.chrome)
            }
            .navigationTitle(tr("Table"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(tr("Save")) {
                        save(columns.map(\.align), head, rows.map(\.cells))
                        dismiss()
                    }
                }
            }
            .onAppear(perform: load)
        }
        .preferredColorScheme(.dark)
    }

    private func cell(_ text: Binding<String>, align: MdAlign, bold: Bool) -> some View {
        TextField("", text: text, axis: .vertical)
            .font(.system(size: 15, weight: bold ? .semibold : .regular))
            .multilineTextAlignment(align == .right ? .trailing : align == .center ? .center : .leading)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .frame(width: 140, alignment: .leading)
            .background(bold ? Dusk.raised : Dusk.surface, in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(Dusk.line))
    }

    private func columnMenu(_ i: Int, _ col: Column) -> some View {
        Menu {
            Picker(tr("Align left"), selection: Binding(get: { columns[i].align }, set: { columns[i].align = $0 })) {
                Label(tr("Align left"), systemImage: "text.alignleft").tag(MdAlign.left)
                Label(tr("Align center"), systemImage: "text.aligncenter").tag(MdAlign.center)
                Label(tr("Align right"), systemImage: "text.alignright").tag(MdAlign.right)
            }
            Button(tr("Insert column left"), systemImage: "arrow.left.to.line") { insertColumn(at: i) }
            Button(tr("Insert column right"), systemImage: "arrow.right.to.line") { insertColumn(at: i + 1) }
            if columns.count > 1 {
                Button(tr("Delete column"), systemImage: "trash", role: .destructive) { deleteColumn(i) }
            }
        } label: {
            Image(systemName: symbol(col.align))
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Dusk.muted)
                .frame(width: 140, height: 22)
        }
        .accessibilityLabel(tr("Column {n}", ["n": "\(i + 1)"]))
    }

    private func rowMenu(_ r: Int) -> some View {
        Menu {
            Button(tr("Insert row above"), systemImage: "arrow.up.to.line") { insertRow(at: r) }
            Button(tr("Insert row below"), systemImage: "arrow.down.to.line") { insertRow(at: r + 1) }
            Button(tr("Duplicate row"), systemImage: "plus.square.on.square") {
                rows.insert(Row(cells: rows[r].cells), at: r + 1)
            }
            Button(tr("Delete row"), systemImage: "trash", role: .destructive) { rows.remove(at: r) }
        } label: {
            Image(systemName: "ellipsis")
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(Dusk.muted)
                .frame(width: 30, height: 36)
        }
        .accessibilityLabel(tr("Row {n}", ["n": "\(r + 1)"]))
    }

    private func symbol(_ a: MdAlign) -> String {
        switch a {
        case .center: return "text.aligncenter"
        case .right: return "text.alignright"
        default: return "text.alignleft"
        }
    }

    private func load() {
        guard !loaded else { return }
        loaded = true
        if let t = original, !t.head.isEmpty {
            columns = t.head.indices.map { Column(align: $0 < t.aligns.count ? t.aligns[$0] : .none) }
            head = t.head
            rows = t.rows.map { Row(cells: $0) }
        } else {
            // A new table: two columns, a row to fill.
            columns = [Column(align: .none), Column(align: .none)]
            head = ["", ""]
            rows = [Row(cells: ["", ""])]
        }
    }

    private func insertRow(at r: Int) {
        rows.insert(Row(cells: Array(repeating: "", count: columns.count)), at: min(r, rows.count))
    }

    private func insertColumn(at i: Int) {
        let i = min(i, columns.count)
        columns.insert(Column(align: .none), at: i)
        head.insert("", at: i)
        for r in rows.indices { rows[r].cells.insert("", at: i) }
    }

    private func deleteColumn(_ i: Int) {
        columns.remove(at: i)
        head.remove(at: i)
        for r in rows.indices { rows[r].cells.remove(at: i) }
    }
}
