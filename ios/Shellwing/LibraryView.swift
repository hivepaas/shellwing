import LocalAuthentication
import PhotosUI
import SwiftUI

/// The Library tab: what the vault keeps besides passwords, as the desktop's
/// Library has it.
struct LibraryView: View {
    enum Part: String, CaseIterable, Identifiable {
        case snippets = "Snippets", envs = "Envs", notes = "Notes", files = "Files", keys = "Keys"
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
                case .files: ConfigFilesList()
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

/// The notes; in a session (`session`), those about its host first, and
/// a tap opens one to read, its commands to insert.
struct NotesList: View {
    @EnvironmentObject private var app: AppModel
    var session: SessionModel?
    @State private var notes: [NoteRow] = []
    @State private var filter = ""

    var body: some View {
        List {
            if notes.isEmpty {
                Text("No notes yet.").foregroundStyle(Dusk.muted).listRowBackground(Dusk.surface)
            }
            ForEach(shown, id: \.id) { n in
                NavigationLink {
                    if let session {
                        NoteReader(note: n, session: session)
                    } else {
                        NoteEditor(note: n)
                    }
                } label: {
                    VStack(alignment: .leading, spacing: 3) {
                        HStack(spacing: 6) {
                            if n.sensitive { Image(systemName: "lock.fill").font(.system(size: 11)).foregroundStyle(Dusk.amber) }
                            Text(n.title.isEmpty ? "Untitled" : n.title).font(.system(size: 15, weight: .medium))
                            if n.here {
                                Text("this host").font(.system(size: 11)).foregroundStyle(Dusk.accent)
                            }
                        }
                        Text(detail(n))
                            .font(.system(size: 12))
                            .foregroundStyle(Dusk.muted)
                            .lineLimit(1)
                    }
                }
                .listRowBackground(Dusk.surface)
                .swipeActions {
                    if session == nil {
                        Button("Delete", role: .destructive) {
                            app.forget(n.id)
                            reload()
                        }
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .searchable(text: $filter, prompt: "Filter notes")
        .toolbar {
            if session == nil {
                NavigationLink {
                    NoteEditor(note: NoteRow(id: "", title: "", category: "", markdown: true, sensitive: false, size: 0, hosts: [], here: false))
                } label: { Image(systemName: "plus") }
                .accessibilityLabel("New note")
            }
        }
        .onAppear(perform: reload)
    }

    private var shown: [NoteRow] {
        let q = filter.lowercased()
        guard !q.isEmpty else { return notes }
        return notes.filter {
            $0.title.lowercased().contains(q) || $0.category.lowercased().contains(q)
                || $0.hosts.contains { $0.lowercased().contains(q) }
        }
    }

    private func detail(_ n: NoteRow) -> String {
        var parts = [n.category.isEmpty ? (n.markdown ? "Markdown" : "Text") : n.category]
        if !n.hosts.isEmpty { parts.append(n.hosts.joined(separator: ", ")) }
        return parts.joined(separator: " · ")
    }

    private func reload() {
        notes = app.notes(for: session?.spec)
    }
}

/// A sensitive note shows once Face ID or the passcode says it is the
/// owner, as on the desktop.
@MainActor
func confirmOwner(_ reason: String, _ then: @escaping (Bool) -> Void) {
    let ctx = LAContext()
    guard ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: nil) else {
        then(true)
        return
    }
    ctx.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { ok, _ in
        Task { @MainActor in then(ok) }
    }
}

/// A note as the desktop's note window has it: written, or shown as
/// Markdown; for hosts; sensitive; with images; exported.
struct NoteEditor: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    let note: NoteRow
    @State private var id = ""
    @State private var title = ""
    @State private var category = ""
    @State private var text = ""
    @State private var markdown = true
    @State private var sensitive = false
    @State private var hosts: [String] = []
    @State private var loaded = false
    /// A sensitive note's text, shown.
    @State private var shown = false
    @State private var preview = false
    @State private var choosingHosts = false
    @State private var picking = false
    @State private var photo: PhotosPickerItem?
    @State private var problem: String?
    @State private var deleted = false
    @State private var confirmDelete = false

    var body: some View {
        VStack(spacing: 0) {
            TextField("Title", text: $title)
                .font(.system(size: 20, weight: .semibold))
                .padding(.horizontal, 16)
                .padding(.top, 10)
            HStack(spacing: 8) {
                TextField("No category", text: $category)
                    .font(.system(size: 13))
                    .foregroundStyle(Dusk.muted)
                if !hosts.isEmpty {
                    Text(hosts.joined(separator: ", "))
                        .font(.system(size: 12))
                        .foregroundStyle(Dusk.accent)
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 8)
            Dusk.line.frame(height: 1)
            if let problem {
                Text(problem)
                    .font(.system(size: 13))
                    .foregroundStyle(Dusk.badText)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
                    .background(Dusk.badBg)
            }
            if !canRead {
                VStack(spacing: 12) {
                    Image(systemName: "lock.fill").font(.system(size: 28)).foregroundStyle(Dusk.amber)
                    Text("Sensitive: its text shows once it is you.").foregroundStyle(Dusk.muted)
                    Button("Show the text") { confirmOwner("Show a sensitive note") { ok in if ok { reveal() } } }
                        .buttonStyle(.bordered)
                }
                .padding(.top, 40)
                Spacer()
            } else if preview && markdown {
                ScrollView {
                    MarkdownView(text: text, insert: nil)
                        .padding(16)
                }
            } else {
                TextEditor(text: $text)
                    .font(.system(size: 15, design: markdown ? .default : .monospaced))
                    .scrollContentBackground(.hidden)
                    .padding(.horizontal, 12)
            }
        }
        .foregroundStyle(Dusk.text)
        .background(Dusk.page.ignoresSafeArea())
        .textInputAutocapitalization(.sentences)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if markdown && canRead {
                ToolbarItem(placement: .primaryAction) {
                    Button(preview ? "Edit" : "Preview") { preview.toggle() }
                }
            }
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Button("For hosts…", systemImage: "server.rack") { choosingHosts = true }
                    Toggle("Sensitive", systemImage: "lock", isOn: $sensitive)
                    Toggle("Markdown", systemImage: "textformat", isOn: $markdown)
                    if canRead {
                        if markdown {
                            Button("Insert an image…", systemImage: "photo") { picking = true }
                        }
                        ShareLink(item: exported, preview: SharePreview(title.isEmpty ? "Note" : title)) {
                            Label("Export…", systemImage: "square.and.arrow.up")
                        }
                    }
                    if !id.isEmpty {
                        Button("Delete", systemImage: "trash", role: .destructive) { confirmDelete = true }
                    }
                } label: { Image(systemName: "ellipsis.circle") }
                .accessibilityLabel("Note")
            }
        }
        .photosPicker(isPresented: $picking, selection: $photo, matching: .images)
        .onChange(of: photo) { _, item in
            guard let item else { return }
            photo = nil
            Task { await insertImage(item) }
        }
        .sheet(isPresented: $choosingHosts) {
            HostsChooser(chosen: $hosts).environmentObject(app)
        }
        .confirmationDialog("Delete \(title.isEmpty ? "this note" : title)?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                app.forget(id)
                deleted = true
                dismiss()
            }
        }
        .onAppear {
            guard !loaded else { return }
            loaded = true
            id = note.id
            title = note.title
            category = note.category
            markdown = note.markdown
            sensitive = note.sensitive
            hosts = note.hosts
            if !note.sensitive { text = note.id.isEmpty ? "" : app.noteText(note.id) }
            preview = note.markdown && !note.id.isEmpty
        }
        .onDisappear(perform: save)
    }

    /// Its text is here to edit: not a sensitive note still hidden.
    private var canRead: Bool { !(note.sensitive && !shown) }

    private var exported: String {
        title.isEmpty ? text : (markdown ? "# \(title)\n\n\(text)" : "\(title)\n\n\(text)")
    }

    private func reveal() {
        text = app.noteText(note.id)
        shown = true
    }

    /// An image from Photos, as JPEG (a photo is HEIC), at the cursor's end.
    private func insertImage(_ item: PhotosPickerItem) async {
        guard let data = try? await item.loadTransferable(type: Data.self),
              let image = UIImage(data: data),
              let jpeg = image.jpegData(compressionQuality: 0.85) else {
            problem = "That image cannot be read."
            return
        }
        do {
            guard let line = try app.core?.addNoteImage(bytes: jpeg) else { return }
            text += (text.isEmpty || text.hasSuffix("\n") ? "" : "\n") + line + "\n"
            problem = nil
        } catch {
            problem = "\(error)"
        }
    }

    private func save() {
        guard !deleted else { return }
        guard !(title.isEmpty && text.isEmpty && id.isEmpty) else { return }
        // A sensitive note not shown was not loaded: its text stays.
        let body: String? = canRead ? text : nil
        switch app.saveNote(
            id: id, title: title, category: category, markdown: markdown,
            hosts: hosts, sensitive: sensitive, text: body
        ) {
        case .success(let saved): id = saved
        case .failure(let e): app.error = "Not saved: \(e)"
        }
    }
}

/// The hosts a note is about.
struct HostsChooser: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    @Binding var chosen: [String]

    var body: some View {
        NavigationStack {
            List {
                let names = Array(Set(app.sections.flatMap(\.hosts).map(\.name) + chosen)).sorted()
                ForEach(names, id: \.self) { name in
                    Button {
                        if let i = chosen.firstIndex(of: name) { chosen.remove(at: i) } else { chosen.append(name) }
                    } label: {
                        HStack {
                            Text(name).foregroundStyle(Dusk.text)
                            Spacer()
                            if chosen.contains(name) { Image(systemName: "checkmark").foregroundStyle(Dusk.accent) }
                        }
                    }
                    .listRowBackground(Dusk.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .navigationTitle("For hosts")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button("Done") { dismiss() } }
        }
        .presentationDetents([.medium, .large])
        .preferredColorScheme(.dark)
    }
}

/// A note read in a session: its commands inserted into the shell.
struct NoteReader: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    let note: NoteRow
    let session: SessionModel
    @State private var text: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text(note.title.isEmpty ? "Untitled" : note.title)
                    .font(.system(size: 22, weight: .semibold))
                if let text {
                    if note.markdown {
                        MarkdownView(text: text) { code in
                            session.paste(code)
                            dismiss()
                        }
                    } else {
                        Text(text).font(.system(size: 14, design: .monospaced)).textSelection(.enabled)
                    }
                } else {
                    Button("Show the text") {
                        confirmOwner("Show a sensitive note") { ok in if ok { text = app.noteText(note.id) } }
                    }
                    .buttonStyle(.bordered)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(16)
        }
        .foregroundStyle(Dusk.text)
        .background(Dusk.page.ignoresSafeArea())
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            if !note.sensitive { text = app.noteText(note.id) }
        }
    }
}
