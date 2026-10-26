import SwiftUI

/// The snippet library, as the desktop's library window has it: ready
/// commands by tool and task, to tick and add to one's snippets, each
/// adjusted first if need be (its options, on the host or in a container,
/// sudo, the environment that has its password).
struct SnippetLibraryView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var items: [LibraryItem] = []
    @State private var query = ""
    @State private var tool: String?
    @State private var task: String?
    @State private var notAdded = false
    @State private var checked: Set<UInt32> = []
    /// How each is to be added, once changed from as it comes.
    @State private var picks: [UInt32: LibraryPick] = [:]
    @AppStorage("library.fileUnder") private var fileUnder = "tool"
    @State private var said: String?
    @State private var problem: String?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    HStack {
                        Menu {
                            Button(tr("Every tool")) { tool = nil }
                            ForEach(tools, id: \.self) { t in Button(t) { tool = t } }
                        } label: { chip(tool ?? tr("Every tool"), on: tool != nil) }
                        Menu {
                            Button(tr("Every task")) { task = nil }
                            ForEach(libraryTasks(), id: \.self) { t in Button(t) { task = t } }
                        } label: { chip(task ?? tr("Every task"), on: task != nil) }
                        Spacer(minLength: 0)
                    }
                    Toggle(tr("Not added yet"), isOn: $notAdded)
                } footer: {
                    Text(tr("Tick the commands to add; each becomes a snippet of your own."))
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)
                if shown.isEmpty {
                    Text(tr("Nothing here: try other words, or every task."))
                        .foregroundStyle(Dusk.muted)
                        .listRowBackground(Dusk.surface)
                }
                ForEach(groups, id: \.0) { group, rows in
                    Section {
                        ForEach(rows, id: \.index) { i in row(i) }
                    } header: {
                        SectionHeading(text: group)
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .searchable(text: $query, prompt: tr("Find a command: a tool, a task, a word…"))
            .navigationTitle(tr("Snippet library"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(tr("Done")) { dismiss() } }
                ToolbarItem(placement: .primaryAction) {
                    Menu {
                        Picker(tr("File under"), selection: $fileUnder) {
                            Text(tr("Its tool (PostgreSQL)")).tag("tool")
                            Text(tr("Its task (Backup)")).tag("task")
                            Text(tr("Both (PostgreSQL · Backup)")).tag("both")
                        }
                    } label: { Image(systemName: "folder") }
                    .accessibilityLabel(tr("File under"))
                }
            }
            .safeAreaInset(edge: .bottom) { bottom }
            .navigationDestination(for: UInt32.self) { n in
                if let i = items.first(where: { $0.index == n }) {
                    LibraryItemView(item: i, pick: pickBinding(i), checked: checkedBinding(n)) { added in
                        if added { reload() }
                    }
                }
            }
            .onAppear(perform: reload)
        }
        .preferredColorScheme(.dark)
    }

    // MARK: Rows

    private func row(_ i: LibraryItem) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Button {
                if checked.contains(i.index) { checked.remove(i.index) } else { checked.insert(i.index) }
            } label: {
                Image(systemName: checked.contains(i.index) ? "checkmark.circle.fill" : "circle")
                    .font(.system(size: 21))
                    .foregroundStyle(checked.contains(i.index) ? Dusk.accent : Dusk.muted)
            }
            .buttonStyle(.borderless)
            .accessibilityLabel(i.name)
            .accessibilityAddTraits(checked.contains(i.index) ? .isSelected : [])
            NavigationLink(value: i.index) {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Text(i.name).font(.system(size: 15, weight: .medium)).foregroundStyle(Dusk.text)
                        if i.changesData {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .font(.system(size: 11))
                                .foregroundStyle(Dusk.amber)
                                .accessibilityLabel(tr("Makes changes"))
                        }
                    }
                    Text(i.about)
                        .font(.system(size: 13))
                        .foregroundStyle(Dusk.soft)
                        .lineLimit(2)
                    HStack(spacing: 6) {
                        badge(i.task, color: Dusk.muted)
                        if i.added > 0 {
                            badge(i.added == 1 ? tr("Added") : tr("Added ×{n}", ["n": "\(i.added)"]), color: Dusk.ok)
                        }
                    }
                }
            }
        }
        .padding(.vertical, 2)
        .listRowBackground(Dusk.surface)
    }

    private func badge(_ text: String, color: Color) -> some View {
        Text(text)
            .font(.system(size: 11))
            .foregroundStyle(color)
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .background(Dusk.chip, in: Capsule())
    }

    private func chip(_ text: String, on: Bool) -> some View {
        HStack(spacing: 4) {
            Text(text).font(.system(size: 14, weight: on ? .semibold : .regular))
            Image(systemName: "chevron.down").font(.system(size: 10, weight: .semibold))
        }
        .padding(.horizontal, 12)
        .frame(height: 32)
        .background(on ? Dusk.selected : Dusk.button, in: Capsule())
        .overlay(Capsule().stroke(on ? Dusk.accent : .clear))
        .foregroundStyle(Dusk.text)
    }

    @ViewBuilder
    private var bottom: some View {
        if let text = said ?? problem {
            Text(text)
                .font(.system(size: 13))
                .foregroundStyle(problem != nil ? Dusk.badText : Dusk.text)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(12)
                .background(Dusk.raised.opacity(0.97))
        } else if !checked.isEmpty {
            HStack {
                Button(tr("Clear")) { checked.removeAll() }
                    .foregroundStyle(Dusk.soft)
                Spacer()
                Button(trN(checked.count, "Add {n} to my snippets", "Add {n} to my snippets")) { add() }
                    .font(.system(size: 15, weight: .semibold))
                    .padding(.horizontal, 16)
                    .frame(height: 40)
                    .background(Dusk.accent, in: Capsule())
                    .foregroundStyle(Dusk.onAccent)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(Dusk.raised.opacity(0.97))
        }
    }

    // MARK: What shows

    private var tools: [String] {
        var seen: [String] = []
        for i in items where !seen.contains(i.tool) { seen.append(i.tool) }
        return seen
    }

    private var shown: [LibraryItem] {
        let words = query.lowercased().split(separator: " ").map(String.init)
        return items.filter { i in
            (tool == nil || i.tool == tool) && (task == nil || i.task == task) && (!notAdded || i.added == 0)
                && words.allSatisfy { w in
                    [i.name, i.about, i.tool, i.task, i.id].contains { $0.lowercased().contains(w) }
                }
        }
    }

    /// By tool, in the library's order.
    private var groups: [(String, [LibraryItem])] {
        var out: [(String, [LibraryItem])] = []
        for i in shown {
            if let k = out.firstIndex(where: { $0.0 == i.tool }) {
                out[k].1.append(i)
            } else {
                out.append((i.tool, [i]))
            }
        }
        return out
    }

    // MARK: Doing

    private var under: FileUnder {
        switch fileUnder {
        case "task": return .task
        case "both": return .both
        default: return .tool
        }
    }

    private func pickBinding(_ i: LibraryItem) -> Binding<LibraryPick> {
        Binding(
            get: { picks[i.index] ?? LibraryPick(index: i.index, options: [], container: false, sudo: nil, env: nil) },
            set: { picks[i.index] = $0 }
        )
    }

    private func checkedBinding(_ n: UInt32) -> Binding<Bool> {
        Binding(
            get: { checked.contains(n) },
            set: { if $0 { checked.insert(n) } else { checked.remove(n) } }
        )
    }

    private func reload() {
        items = app.core?.libraryItems() ?? []
    }

    private func add() {
        let chosen = checked.sorted().map {
            picks[$0] ?? LibraryPick(index: $0, options: [], container: false, sudo: nil, env: nil)
        }
        do {
            let n = try app.core?.addFromLibrary(picks: chosen, under: under) ?? 0
            checked.removeAll()
            problem = nil
            show(trN(Int(n), "{n} snippet added: run it from a session's Snippets key.", "{n} snippets added: run them from a session's Snippets key."))
            reload()
        } catch {
            problem = "\(error)"
        }
    }

    private func show(_ text: String) {
        said = text
        Task {
            try? await Task.sleep(for: .seconds(4))
            if said == text { said = nil }
        }
    }
}

/// One ready command: what it runs, and how it is to be added.
struct LibraryItemView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    let item: LibraryItem
    @Binding var pick: LibraryPick
    @Binding var checked: Bool
    /// Told when it was added on its own.
    let done: (Bool) -> Void
    @AppStorage("library.fileUnder") private var fileUnder = "tool"
    @State private var problem: String?

    var body: some View {
        Form {
            Section {
                Text(item.about).foregroundStyle(Dusk.label)
                Text(libraryCommand(pick: pick))
                    .font(.system(size: 13, design: .monospaced))
                    .foregroundStyle(Dusk.text)
                    .textSelection(.enabled)
                if let docs = item.docs, let url = URL(string: docs) {
                    Link(tr("Docs ↗"), destination: url)
                }
            } header: {
                SectionHeading(text: "\(item.tool) · \(item.task)")
            } footer: {
                VStack(alignment: .leading, spacing: 6) {
                    if item.changesData {
                        Text(tr("It makes changes: added, it is typed and waits for you to press Enter."))
                    }
                    let asked = snippetAsked(command: libraryCommand(pick: pick))
                    if !asked.isEmpty {
                        Text(tr("Asked when it runs: {names}. Once added, it is yours to edit.", ["names": asked.joined(separator: ", ")]))
                    }
                }
                .foregroundStyle(Dusk.muted)
            }
            .listRowBackground(Dusk.surface)

            if !item.options.isEmpty {
                Section {
                    ForEach(Array(item.options.enumerated()), id: \.offset) { n, option in
                        Toggle(isOn: Binding(
                            get: { pick.options.contains(UInt32(n)) },
                            set: { on in
                                pick.options.removeAll { $0 == UInt32(n) }
                                if on { pick.options.append(UInt32(n)); pick.options.sort() }
                            }
                        )) {
                            Text(option).font(.system(size: 14, design: .monospaced))
                        }
                    }
                } header: {
                    SectionHeading(text: tr("Options"))
                }
                .listRowBackground(Dusk.surface)
            }

            Section {
                if item.canContain {
                    Picker(tr("Runs"), selection: $pick.container) {
                        Text(tr("On the host")).tag(false)
                        Text(tr("In a Docker container: asks which")).tag(true)
                    }
                }
                Toggle(tr("Run with sudo"), isOn: Binding(
                    get: { pick.sudo ?? item.sudo },
                    set: { pick.sudo = $0 }
                ))
            } footer: {
                VStack(alignment: .leading, spacing: 6) {
                    if pick.container && item.canContain {
                        Text(tr("The files it reads or writes are the container's."))
                    }
                    if item.sudo { Text(tr("it needs root")) }
                }
                .foregroundStyle(Dusk.muted)
            }
            .listRowBackground(Dusk.surface)

            if let variable = item.password {
                Section {
                    Picker(tr("Password"), selection: envBinding) {
                        Text(tr("None: it asks, or goes without")).tag("")
                        ForEach(app.environments(), id: \.id) { e in
                            Text(e.names.contains(variable) ? "\(e.name) · \(tr("has {var}", ["var": variable]))" : e.name).tag(e.id)
                        }
                    }
                } header: {
                    SectionHeading(text: tr("Password"))
                } footer: {
                    Text(app.environments().isEmpty
                        ? tr("None yet: add one in Library › Envs.")
                        : tr("It reads {var}: give it an environment that has it.", ["var": variable]))
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)
            }

            if let problem {
                Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
            }

            Section {
                Toggle(tr("Picked"), isOn: $checked)
                Button(tr("Add to my snippets")) { add() }
                    .font(.system(size: 16, weight: .semibold))
            }
            .listRowBackground(Dusk.surface)
        }
        .scrollContentBackground(.hidden)
        .background(Dusk.page.ignoresSafeArea())
        .navigationTitle(item.name)
        .navigationBarTitleDisplayMode(.inline)
    }

    /// The environment for its password: the one picked, else the one found.
    private var envBinding: Binding<String> {
        Binding(
            get: { pick.env ?? app.core?.libraryEnv(index: item.index) ?? "" },
            set: { pick.env = $0 }
        )
    }

    private func add() {
        let under: FileUnder = fileUnder == "task" ? .task : fileUnder == "both" ? .both : .tool
        do {
            _ = try app.core?.addFromLibrary(picks: [pick], under: under)
            checked = false
            done(true)
            dismiss()
        } catch {
            problem = "\(error)"
        }
    }
}
