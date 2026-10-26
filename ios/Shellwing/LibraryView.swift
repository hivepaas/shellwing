import LocalAuthentication
import SwiftUI

/// The Library tab: what the vault keeps besides passwords, as the desktop's
/// Library has it.
struct LibraryView: View {
    enum Part: String, CaseIterable, Identifiable {
        case snippets = "Snippets", envs = "Environments", notes = "Notes", keys = "Keys"
        var id: String { rawValue }
    }

    @State private var part: Part = .snippets

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                Picker("Part", selection: $part) {
                    ForEach(Part.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .padding(.horizontal, 16)
                .padding(.bottom, 8)
                switch part {
                case .snippets: SnippetsList(spec: nil, run: nil)
                case .envs: EnvironmentsList()
                case .notes: NotesList()
                case .keys: KeysView()
                }
            }
            .background(Dusk.panel.ignoresSafeArea())
            .navigationTitle("Library")
        }
    }
}

// MARK: Snippets

/// Snippets by category. With `run`, a tap runs one (in a session);
/// without, it opens the editor.
struct SnippetsList: View {
    @EnvironmentObject private var app: AppModel
    let spec: String?
    let run: ((SnippetRow) -> Void)?
    @State private var snippets: [SnippetRow] = []
    @State private var editing: SnippetEdit?
    @State private var filter = ""

    var body: some View {
        List {
            if snippets.isEmpty {
                Text(spec == nil
                    ? "No snippets yet. Make one with +, or sync those of your other devices."
                    : "No snippets for this host.")
                    .foregroundStyle(Dusk.muted)
                    .listRowBackground(Dusk.surface)
            }
            ForEach(categories, id: \.self) { cat in
                Section {
                    ForEach(shown.filter { $0.category == cat }, id: \.id) { s in
                        Button {
                            if let run { run(s) } else { editing = edit(s) }
                        } label: {
                            VStack(alignment: .leading, spacing: 3) {
                                Text(s.name).font(.system(size: 15, weight: .medium)).foregroundStyle(Dusk.text)
                                Text(s.oneLine)
                                    .font(.system(size: 12, design: .monospaced))
                                    .foregroundStyle(Dusk.soft)
                                    .lineLimit(2)
                            }
                        }
                        .listRowBackground(Dusk.surface)
                        .swipeActions {
                            Button("Delete", role: .destructive) {
                                app.forget(s.id)
                                reload()
                            }
                            Button("Edit") { editing = edit(s) }.tint(Dusk.button)
                        }
                    }
                } header: {
                    SectionHeading(text: cat.isEmpty ? "No category" : cat)
                }
            }
        }
        .scrollContentBackground(.hidden)
        .searchable(text: $filter, prompt: "Filter snippets")
        .toolbar {
            Button {
                editing = SnippetEdit(id: "", name: "", category: "", command: "", enter: true, scope: "")
            } label: { Image(systemName: "plus") }
            .accessibilityLabel("New snippet")
        }
        .sheet(item: $editing, onDismiss: reload) { e in
            SnippetEditor(edit: e)
        }
        .onAppear(perform: reload)
    }

    private var shown: [SnippetRow] {
        let q = filter.lowercased()
        guard !q.isEmpty else { return snippets }
        return snippets.filter { $0.name.lowercased().contains(q) || $0.command.lowercased().contains(q) }
    }

    private var categories: [String] {
        var seen: [String] = []
        for s in shown where !seen.contains(s.category) { seen.append(s.category) }
        return seen
    }

    private func reload() {
        snippets = app.snippets(for: spec)
    }

    private func edit(_ s: SnippetRow) -> SnippetEdit {
        SnippetEdit(id: s.id, name: s.name, category: s.category, command: s.command, enter: s.enter, scope: s.scope)
    }
}

extension SnippetEdit: Identifiable {}

struct SnippetEditor: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    @State var edit: SnippetEdit

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $edit.name)
                    TextField("Category", text: $edit.category)
                }
                .listRowBackground(Dusk.surface)
                Section {
                    TextEditor(text: $edit.command)
                        .font(.system(size: 14, design: .monospaced))
                        .frame(minHeight: 120)
                    Toggle("Press Enter after it", isOn: $edit.enter)
                } header: {
                    SectionHeading(text: "Command")
                } footer: {
                    Text("{{name}} asks for a value when it runs; {{host}}, {{user}} and {{address}} come from the session.")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)
                Section {
                    Picker("Offered", selection: $edit.scope) {
                        Text("Everywhere").tag("")
                        ForEach(app.groups(), id: \.self) { g in Text("Group: \(g)").tag("group:\(g)") }
                        ForEach(app.sections.flatMap(\.hosts), id: \.id) { h in
                            Text("Host: \(h.name)").tag("host:\(h.name)")
                        }
                    }
                }
                .listRowBackground(Dusk.surface)
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .navigationTitle(edit.id.isEmpty ? "New snippet" : edit.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { if app.save(snippet: edit) { dismiss() } }
                        .disabled(edit.name.isEmpty || edit.command.isEmpty)
                }
            }
        }
        .preferredColorScheme(.dark)
    }
}

/// Asks a snippet's holes, then runs it in the session.
struct RunSnippetSheet: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    let snippet: SnippetRow
    let session: SessionModel
    @State private var values: [String: String] = [:]
    @State private var problem: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text(snippet.command)
                        .font(.system(size: 13, design: .monospaced))
                        .foregroundStyle(Dusk.soft)
                }
                .listRowBackground(Dusk.surface)
                if !snippet.asked.isEmpty {
                    Section {
                        ForEach(snippet.asked, id: \.self) { name in
                            TextField(name, text: Binding(
                                get: { values[name] ?? "" },
                                set: { values[name] = $0 }
                            ))
                            .font(.system(.body, design: .monospaced))
                        }
                    } header: {
                        SectionHeading(text: "Values")
                    }
                    .listRowBackground(Dusk.surface)
                }
                if let problem {
                    Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .navigationTitle(snippet.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Run") {
                        if let why = app.run(snippet: snippet.id, in: session, values: values) {
                            problem = why
                        } else {
                            dismiss()
                        }
                    }
                }
            }
        }
        .preferredColorScheme(.dark)
    }
}

extension SnippetRow: Identifiable {}

// MARK: Environments

struct EnvironmentsList: View {
    @EnvironmentObject private var app: AppModel
    @State private var envs: [EnvRow] = []
    @State private var editing: EnvRow?

    var body: some View {
        List {
            if envs.isEmpty {
                Text("No environments yet: variables kept in the vault, loaded into a session's shell without showing their values.")
                    .foregroundStyle(Dusk.muted)
                    .listRowBackground(Dusk.surface)
            }
            ForEach(envs, id: \.id) { e in
                Button { editing = e } label: {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(e.name).font(.system(size: 15, weight: .medium)).foregroundStyle(Dusk.text)
                        Text(e.names.joined(separator: " "))
                            .font(.system(size: 12, design: .monospaced))
                            .foregroundStyle(Dusk.muted)
                            .lineLimit(2)
                    }
                }
                .listRowBackground(Dusk.surface)
                .swipeActions {
                    Button("Delete", role: .destructive) {
                        app.forget(e.id)
                        envs = app.environments()
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .toolbar {
            Button { editing = EnvRow(id: "", name: "", scope: "", names: []) } label: { Image(systemName: "plus") }
                .accessibilityLabel("New environment")
        }
        .sheet(item: $editing, onDismiss: { envs = app.environments() }) { e in
            EnvEditor(env: e)
        }
        .onAppear { envs = app.environments() }
    }
}

extension EnvRow: Identifiable {}

/// An environment as `.env` text. Its variables are listed without values:
/// a name left so keeps the one it has; a value typed replaces it.
struct EnvEditor: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    let env: EnvRow
    @State private var name = ""
    @State private var text = ""

    var body: some View {
        NavigationStack {
            Form {
                Section { TextField("Name", text: $name) }
                    .listRowBackground(Dusk.surface)
                Section {
                    TextEditor(text: $text)
                        .font(.system(size: 14, design: .monospaced))
                        .frame(minHeight: 180)
                } header: {
                    SectionHeading(text: "Variables")
                } footer: {
                    Text("NAME=value, a line. A name with nothing after = keeps the value it has; values never show here.")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .navigationTitle(env.id.isEmpty ? "New environment" : env.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        if app.saveEnvironment(id: env.id, name: name, scope: env.scope, dotenv: text) { dismiss() }
                    }
                    .disabled(name.isEmpty)
                }
            }
            .onAppear {
                name = env.name
                text = env.names.map { "\($0)=" }.joined(separator: "\n")
            }
        }
        .preferredColorScheme(.dark)
    }
}

// MARK: Notes

struct NotesList: View {
    @EnvironmentObject private var app: AppModel
    @State private var notes: [NoteRow] = []

    var body: some View {
        List {
            if notes.isEmpty {
                Text("No notes yet.").foregroundStyle(Dusk.muted).listRowBackground(Dusk.surface)
            }
            ForEach(notes, id: \.id) { n in
                NavigationLink {
                    NoteEditor(note: n)
                } label: {
                    VStack(alignment: .leading, spacing: 3) {
                        HStack(spacing: 6) {
                            if n.sensitive { Image(systemName: "lock.fill").font(.system(size: 11)).foregroundStyle(Dusk.amber) }
                            Text(n.title.isEmpty ? "Untitled" : n.title).font(.system(size: 15, weight: .medium))
                        }
                        Text(n.category.isEmpty ? (n.markdown ? "Markdown" : "Text") : n.category)
                            .font(.system(size: 12))
                            .foregroundStyle(Dusk.muted)
                    }
                }
                .listRowBackground(Dusk.surface)
                .swipeActions {
                    Button("Delete", role: .destructive) {
                        app.forget(n.id)
                        notes = app.notes()
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .toolbar {
            NavigationLink {
                NoteEditor(note: NoteRow(id: "", title: "", category: "", markdown: true, sensitive: false, size: 0))
            } label: { Image(systemName: "plus") }
            .accessibilityLabel("New note")
        }
        .onAppear { notes = app.notes() }
    }
}

struct NoteEditor: View {
    @EnvironmentObject private var app: AppModel
    let note: NoteRow
    @State private var id = ""
    @State private var title = ""
    @State private var text = ""
    @State private var loaded = false
    @State private var shown = false

    var body: some View {
        VStack(spacing: 0) {
            TextField("Title", text: $title)
                .font(.system(size: 20, weight: .semibold))
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
            Dusk.line.frame(height: 1)
            if note.sensitive && !shown {
                Button("Show the note") { confirm() }
                    .buttonStyle(.bordered)
                    .padding(.top, 40)
                Spacer()
            } else {
                TextEditor(text: $text)
                    .font(.system(size: 15, design: note.markdown ? .default : .monospaced))
                    .scrollContentBackground(.hidden)
                    .padding(.horizontal, 12)
            }
        }
        .foregroundStyle(Dusk.text)
        .background(Dusk.page.ignoresSafeArea())
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            guard !loaded else { return }
            loaded = true
            id = note.id
            title = note.title
            text = note.id.isEmpty ? "" : app.noteText(note.id)
        }
        .onDisappear(perform: save)
    }

    /// A sensitive note shows once Face ID or the passcode says it is the
    /// owner, as on the desktop.
    private func confirm() {
        let ctx = LAContext()
        guard ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: nil) else {
            shown = true
            return
        }
        ctx.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: "Show a sensitive note") { ok, _ in
            Task { @MainActor in shown = ok }
        }
    }

    private func save() {
        guard !(title.isEmpty && text.isEmpty) else { return }
        // A sensitive note not shown was not loaded: nothing to write over it.
        guard !(note.sensitive && !shown) else { return }
        if let saved = app.saveNote(id: id, title: title, category: note.category, markdown: note.markdown, text: text) {
            id = saved
        }
    }
}
