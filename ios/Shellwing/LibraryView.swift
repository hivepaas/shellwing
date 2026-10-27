import LocalAuthentication
import PhotosUI
import SwiftUI

/// The Library tab: what the vault keeps besides passwords, as the desktop's
/// Library has it.
struct LibraryView: View {
    @EnvironmentObject private var app: AppModel

    enum Part: String, CaseIterable, Identifiable {
        case snippets = "Snippets", envs = "Envs", notes = "Notes", files = "Files", keys = "Keys"
        var id: String { rawValue }
        var title: String {
            switch self {
            case .snippets: return tr("Snippets")
            case .envs: return tr("Envs")
            case .notes: return tr("Notes")
            case .files: return tr("Files")
            case .keys: return tr("Keys")
            }
        }
    }

    @State private var part: Part = .snippets
    #if DEBUG
    /// `-screen env`: the sample environment's editor.
    @State private var envSample: EnvRow?
    #endif

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                Picker(tr("Part"), selection: $part) {
                    ForEach(Part.allCases) { Text($0.title).tag($0) }
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
            .navigationTitle(tr("Library"))
            #if DEBUG
            // `-screen markdown`: how a note shows, the desktop's template
            // and what it lacks (a table, alerts, nested lists).
            .sheet(isPresented: .constant(UserDefaults.standard.string(forKey: "screen") == "mdedit")) {
                MarkdownEditorSample().environmentObject(app).preferredColorScheme(.dark)
            }
            .sheet(isPresented: .constant(UserDefaults.standard.string(forKey: "screen") == "markdown")) {
                ScrollView {
                    MarkdownView(text: MarkdownSample.text, toggle: { _ in })
                        .padding(16)
                }
                .background(Dusk.page.ignoresSafeArea())
                .environmentObject(app)
                .preferredColorScheme(.dark)
            }
            .sheet(item: $envSample) { e in EnvEditor(env: e).environmentObject(app) }
            .onAppear {
                if UserDefaults.standard.string(forKey: "screen")?.hasPrefix("env") == true {
                    part = .envs
                    envSample = EnvSample.make(app)
                }
            }
            #endif
        }
    }
}

#if DEBUG
/// `-screen env`: the editor on a sample environment whose values need
/// quotes to read back; `-showValues YES` shows them at once. `-screen
/// envgone`: the sample deleted.
enum EnvSample {
    static let name = "Debug sample"

    @MainActor
    static func make(_ app: AppModel) -> EnvRow? {
        let old = app.environments().first { $0.name == name }
        if UserDefaults.standard.string(forKey: "screen") == "envgone" {
            if let old { app.forget(old.id) }
            return nil
        }
        app.saveEnvironment(id: old?.id ?? "", name: name, scope: "", dotenv: "API_TOKEN=tok-123\nPADDED=\" spaced \"\nQUOTED='\"q\"'\n")
        return app.environments().first { $0.name == name }
    }
}
#endif

/// A table opened in its grid: nil, a new one.
struct TableSheet: Identifiable {
    let id = UUID()
    let table: MdTable?
}

#if DEBUG
/// `-screen mdedit`: the editor on the sample, the caret in it.
struct MarkdownEditorSample: View {
    /// What the sample's image shows: a picture drawn here.
    static let picture: UIImage = UIGraphicsImageRenderer(size: CGSize(width: 600, height: 300)).image { c in
        let colors = [UIColor(hex: 0x4cc4f9).cgColor, UIColor(hex: 0x22c55e).cgColor] as CFArray
        let g = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: colors, locations: [0, 1])!
        c.cgContext.drawLinearGradient(g, start: .zero, end: CGPoint(x: 600, y: 300), options: [])
        ("web-1 · 22" as NSString).draw(at: CGPoint(x: 30, y: 120), withAttributes: [
            .font: UIFont.monospacedSystemFont(ofSize: 48, weight: .bold), .foregroundColor: UIColor.white,
        ])
    }

    @State private var text = MarkdownSample.text
    @StateObject private var editor = MarkdownEditorController()
    @State private var tabling: TableSheet?

    var body: some View {
        MarkdownEditor(text: $text, controller: editor)
            .background(Dusk.page.ignoresSafeArea())
            .onAppear {
                editor.imageFor = { _ in Self.picture }
                editor.onTable = { table in tabling = TableSheet(table: table) }
            }
            .sheet(item: $tabling) { sheet in
                TableEditor(original: sheet.table) { aligns, head, rows in
                    editor.setTable(sheet.table, aligns: aligns, head: head, rows: rows)
                }
            }
            .task {
                try? await Task.sleep(for: .milliseconds(600))
                guard let tv = editor.textView else { return }
                // `-caret <offset>` (or `end`): where the caret starts;
                // `-caret none`: the keyboard down.
                let caret = UserDefaults.standard.string(forKey: "caret") ?? "40"
                guard caret != "none" else { return }
                tv.becomeFirstResponder()
                let n = (tv.text as NSString).length
                var at = caret == "end" ? n : min(Int(caret) ?? 40, n)
                // `-caretAt <words>`: the caret at those words.
                if let words = UserDefaults.standard.string(forKey: "caretAt") {
                    let r = (tv.text as NSString).range(of: words)
                    if r.location != NSNotFound { at = r.location }
                }
                tv.selectedRange = NSRange(location: at, length: caret == "40" ? 4 : 0)
                tv.scrollRangeToVisible(tv.selectedRange)
                if UserDefaults.standard.string(forKey: "script") == "check" { await check(tv) }
                if UserDefaults.standard.string(forKey: "script") == "table" {
                    select(tv, "web-1 |")
                    editor.table()
                }
            }
    }

    private func select(_ tv: UITextView, _ what: String) {
        let r = (tv.text as NSString).range(of: what)
        if r.location != NSNotFound { tv.selectedRange = NSRange(location: r.location, length: 0) }
    }

    /// `-script check`: the bar's edits, Return and Undo, done on the text
    /// view as a user would; what came of each written to `editor-check.txt`
    /// in the app's tmp, to read.
    private func check(_ tv: UITextView) async {
        var said: [String] = []
        func select(_ what: String, offset: Int = 0, length: Int? = nil) {
            let r = (tv.text as NSString).range(of: what)
            tv.selectedRange = NSRange(location: r.location + offset, length: length ?? r.length)
        }
        /// As the keyboard types: the delegate asked first.
        func type(_ t: String) {
            let r = tv.selectedRange
            if tv.delegate?.textView?(tv, shouldChangeTextIn: r, replacementText: t) ?? true {
                tv.insertText(t)
            }
        }
        func pause() async { try? await Task.sleep(for: .milliseconds(150)) }
        func line(_ what: String) -> String {
            (tv.text ?? "").components(separatedBy: "\n").first { $0.contains(what) } ?? "(gone)"
        }
        select("load")
        editor.wrap(.bold)
        await pause()
        said.append("bold: " + line("load") + " | selected: " + (tv.text as NSString).substring(with: tv.selectedRange))
        select("Tell the team", offset: 13, length: 0)
        type("\n")
        await pause()
        type("Ring Linh")
        await pause()
        said.append("return: " + line("Ring Linh"))
        type("\n")
        type("\n")
        said.append("empty item ends: [" + line("Ring Linh") + "] then [" + (tv.text as NSString).substring(with: (tv.text as NSString).lineRange(for: tv.selectedRange)).replacingOccurrences(of: "\n", with: "⏎") + "]")
        select("Pull the image", offset: 0, length: 0)
        editor.indent(out: false)
        await pause()
        said.append("indent: " + line("Pull the image"))
        editor.undo()
        await pause()
        said.append("undo: " + line("Pull the image") + " | bold kept: " + line("load"))
        select("Check the logs", offset: 0, length: 0)
        editor.lines(.task)
        await pause()
        said.append("task: " + line("Check the logs"))
        select("Deploying", offset: 0, length: 0)
        editor.lines(.heading)
        await pause()
        said.append("heading: " + line("Deploying"))
        editor.insertBlock("![](attachment:abc)")
        await pause()
        said.append("image after: " + (tv.text ?? "").components(separatedBy: "\n").prefix(2).joined(separator: " ⏎ "))
        select("web-1 |", offset: 0, length: 0)
        if let t = mdTableAt(text: tv.text, at: UInt32(tv.selectedRange.location)) {
            var rows = t.rows
            rows[0][1] = "2222"
            rows.append(["db-replica", "5433", "database"])
            editor.setTable(t, aligns: t.aligns, head: t.head, rows: rows)
            await pause()
            let lines = (tv.text ?? "").components(separatedBy: "\n").filter { $0.hasPrefix("|") }
            said.append("table: " + lines.joined(separator: " ⏎ "))
            editor.undo()
            await pause()
            said.append("table undone: " + ((tv.text ?? "").contains("db-replica") ? "no" : "yes"))
        }
        let boxes = markdownRuns(text: tv.text).filter { $0.role == .taskBox }
        if let box = boxes.first {
            let ns = { (tv.text ?? "") as NSString }
            let lineOf = { ns().substring(with: ns().lineRange(for: NSRange(location: Int(box.start), length: 0))).trimmingCharacters(in: .newlines) }
            let before = lineOf()
            editor.toggleTask(box: Int(box.start))
            await pause()
            let ticked = lineOf()
            editor.undo()
            await pause()
            said.append("tick: [" + before + "] -> [" + ticked + "] -> undo [" + lineOf() + "]")
        }
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("editor-check.txt")
        try? said.joined(separator: "\n").write(to: url, atomically: true, encoding: .utf8)
    }
}

enum MarkdownSample {
    static let text = """
    # Deploying web-1

    Check **disk** and *load* first, ~~then reboot~~, with `df -h`. Docs: https://example.com/runbook_v2.

    > [!WARNING]
    > Restarting nginx drops open connections.

    > [!TIP] Run it inside tmux.

    1. Pull the image
    2. Restart
       - nginx
       - the workers
    3. Check the logs

    - [x] Back up the database
    - [ ] Tell the team

    ![The dashboard](attachment:sample)

    | Host | Port | Role |
    |:--|--:|:-:|
    | web-1 | 22 | front |
    | db-primary | 5432 | database |

    ```bash
    sudo systemctl restart nginx
    journalctl -u nginx --since "10 min ago"
    ```

    > A plain quote
    > > and one inside it.

    ---

    Done.
    """
}
#endif

// MARK: Snippets

/// Snippets by category. With `run`, a tap runs one (in a session);
/// without, it opens the editor.
struct SnippetsList: View {
    @EnvironmentObject private var app: AppModel
    let spec: String?
    let run: ((SnippetRow) -> Void)?
    @State private var snippets: [SnippetRow] = []
    @State private var editing: SnippetEdit?
    @State private var doomed: Doomed?
    @State private var browsing = UserDefaults.standard.string(forKey: "screen") == "library"
    @State private var filter = ""

    var body: some View {
        List {
            if snippets.isEmpty {
                Text(spec == nil
                    ? tr("No snippets yet. Make one with +, or sync those of your other devices.")
                    : tr("No snippets for this host."))
                    .foregroundStyle(Dusk.muted)
                    .listRowBackground(Dusk.surface)
                if run == nil {
                    Button(tr("Browse the snippet library"), systemImage: "books.vertical") { browsing = true }
                        .listRowBackground(Dusk.surface)
                }
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
                        .confirming($doomed, about: s.id)
                        .swipeActions {
                            DeleteSwipe {
                                doomed = Doomed(key: s.id, title: tr("Delete {name}?", ["name": s.name]), message: goesEverywhere()) {
                                    app.forget(s.id)
                                    reload()
                                }
                            }
                            Button(tr("Edit")) { editing = edit(s) }.tint(Dusk.button)
                        }
                    }
                } header: {
                    SectionHeading(text: cat.isEmpty ? tr("No category") : cat)
                }
            }
        }
        .scrollContentBackground(.hidden)
        .searchable(text: $filter, prompt: tr("Filter snippets"))
        .toolbar {
            if run == nil {
                Button { browsing = true } label: { Image(systemName: "books.vertical") }
                    .accessibilityLabel(tr("Snippet library"))
            }
            Button {
                editing = SnippetEdit(id: "", name: "", category: "", command: "", enter: true, scope: "", needs: [])
            } label: { Image(systemName: "plus") }
            .accessibilityLabel(tr("New snippet"))
        }
        .sheet(item: $editing, onDismiss: reload) { e in
            SnippetEditor(edit: e)
        }
        .sheet(isPresented: $browsing, onDismiss: reload) {
            SnippetLibraryView().environmentObject(app)
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
        SnippetEdit(id: s.id, name: s.name, category: s.category, command: s.command, enter: s.enter, scope: s.scope, needs: s.needs)
    }
}

extension SnippetEdit: Identifiable {}

/// Where a snippet, an environment or a config file is offered: everywhere,
/// in a group's sessions, or one host's.
struct ScopePicker: View {
    @EnvironmentObject private var app: AppModel
    @Binding var scope: String

    var body: some View {
        Picker(tr("Offered"), selection: $scope) {
            Text(tr("Everywhere")).tag("")
            ForEach(app.groups(), id: \.self) { g in Text(tr("Group: {group}", ["group": g])).tag("group:\(g)") }
            ForEach(hostNames, id: \.self) { h in Text(tr("Host: {host}", ["host": h])).tag("host:\(h)") }
            // One no longer here (synced, or renamed): kept as it is.
            if !scope.isEmpty, !known { Text(scope).tag(scope) }
        }
    }

    private var hostNames: [String] {
        Array(Set(app.sections.flatMap(\.hosts).map(\.name))).sorted()
    }

    private var known: Bool {
        app.groups().map { "group:\($0)" }.contains(scope) || hostNames.map { "host:\($0)" }.contains(scope)
    }
}

struct SnippetEditor: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    @State var edit: SnippetEdit

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField(tr("Name"), text: $edit.name)
                    TextField(tr("Category"), text: $edit.category)
                }
                .listRowBackground(Dusk.surface)
                Section {
                    TextEditor(text: $edit.command)
                        .font(.system(size: 14, design: .monospaced))
                        .frame(minHeight: 120)
                    Toggle(tr("Press Enter after it"), isOn: $edit.enter)
                } header: {
                    SectionHeading(text: tr("Command"))
                } footer: {
                    Text(tr("{{name}} asks for a value when it runs; {{host}}, {{user}} and {{address}} come from the session."))
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)
                Section {
                    ScopePicker(scope: $edit.scope)
                }
                .listRowBackground(Dusk.surface)
                let envs = app.environments()
                if !envs.isEmpty {
                    Section {
                        ForEach(envs, id: \.id) { e in
                            Toggle(isOn: Binding(
                                get: { edit.needs.contains(e.id) },
                                set: { on in
                                    edit.needs.removeAll { $0 == e.id }
                                    if on { edit.needs.append(e.id) }
                                }
                            )) {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(e.name)
                                    Text(e.names.joined(separator: " "))
                                        .font(.system(size: 11, design: .monospaced))
                                        .foregroundStyle(Dusk.muted)
                                        .lineLimit(1)
                                }
                            }
                        }
                    } header: {
                        SectionHeading(text: tr("Runs with"))
                    } footer: {
                        Text(tr("Their variables are given to it alone, in a subshell, without showing their values."))
                            .foregroundStyle(Dusk.muted)
                    }
                    .listRowBackground(Dusk.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .navigationTitle(edit.id.isEmpty ? tr("New snippet") : edit.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(tr("Save")) { if app.save(snippet: edit) { dismiss() } }
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
                        SectionHeading(text: tr("Values"))
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
                ToolbarItem(placement: .cancellationAction) { Button(tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(tr("Run")) {
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
    @State private var doomed: Doomed?

    var body: some View {
        List {
            if envs.isEmpty {
                Text(tr("No environments yet: variables kept in the vault, loaded into a session's shell without showing their values."))
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
                .confirming($doomed, about: e.id)
                .swipeActions {
                    DeleteSwipe {
                        doomed = Doomed(key: e.id, title: tr("Delete {name}?", ["name": e.name]), message: goesEverywhere()) {
                            app.forget(e.id)
                            envs = app.environments()
                        }
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .toolbar {
            Button { editing = EnvRow(id: "", name: "", scope: "", names: []) } label: { Image(systemName: "plus") }
                .accessibilityLabel(tr("New environment"))
        }
        .sheet(item: $editing, onDismiss: { envs = app.environments() }) { e in
            EnvEditor(env: e)
        }
        .onAppear { envs = app.environments() }
    }
}

extension EnvRow: Identifiable {}

/// An environment as `.env` text. Its variables are listed without values:
/// a name left so keeps the one it has; a value typed replaces it. Show
/// values puts them in, once Face ID or the passcode says it is the owner,
/// as the desktop's eyes do, for a minute.
struct EnvEditor: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase
    let env: EnvRow
    @State private var name = ""
    @State private var scope = ""
    @State private var text = ""
    /// The values kept are in the text, for a while.
    @State private var shown = false
    /// Hides them when the while is over.
    @State private var hiding: Task<Void, Never>?
    /// Variables whose value is of several lines, left out of the text.
    @State private var notShown: [String] = []
    @State private var problem: String?

    /// How long values stay shown: the desktop's minute.
    private static let shownFor: Duration = .seconds(60)

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField(tr("Name"), text: $name)
                    ScopePicker(scope: $scope)
                }
                .listRowBackground(Dusk.surface)
                Section {
                    TextEditor(text: $text)
                        .font(.system(size: 14, design: .monospaced))
                        .frame(minHeight: 180)
                } header: {
                    HStack {
                        SectionHeading(text: tr("Variables"))
                        Spacer()
                        if !env.id.isEmpty {
                            Button {
                                if shown { hide() } else { askToShow() }
                            } label: {
                                Label(shown ? tr("Hide values") : tr("Show values"), systemImage: shown ? "eye.slash" : "eye")
                                    .font(.system(size: 13, weight: .semibold))
                            }
                            .foregroundStyle(Dusk.accent)
                            .textCase(nil)
                        }
                    }
                } footer: {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(tr("NAME=value, a line. A name with nothing after = keeps the value it has."))
                        if !env.id.isEmpty {
                            Text(tr("Values shown hide again after a minute, or when you leave the app."))
                        }
                        if !notShown.isEmpty {
                            Text(tr("Not shown, of several lines: {names}. The desktop shows them.", ["names": notShown.joined(separator: ", ")]))
                        }
                        if let problem {
                            Text(problem).foregroundStyle(Dusk.badText)
                        }
                    }
                    .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .navigationTitle(env.id.isEmpty ? tr("New environment") : env.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(tr("Cancel")) { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(tr("Save")) {
                        // What was shown and not changed is kept, not saved again.
                        let dotenv = shown ? app.envTextHidden(id: env.id, text: text) : text
                        if app.saveEnvironment(id: env.id, name: name, scope: scope, dotenv: dotenv) { dismiss() }
                    }
                    .disabled(name.isEmpty)
                }
            }
            .onAppear {
                name = env.name
                scope = env.scope
                text = env.names.map { "\($0)=" }.joined(separator: "\n")
                #if DEBUG
                // Not asked: a script cannot type the Simulator's passcode.
                if UserDefaults.standard.bool(forKey: "showValues") { show() }
                #endif
            }
            // Out of sight, out of the app switcher's picture too.
            .onChange(of: phase) { _, now in
                if now != .active { hide() }
            }
            .onDisappear { hiding?.cancel() }
        }
        .preferredColorScheme(.dark)
    }

    /// Shows the values once Face ID or the passcode says it is the owner.
    private func askToShow() {
        confirmOwner(tr("Show the values of {name}", ["name": env.name])) { ok in if ok { show() } }
    }

    private func show() {
        problem = nil
        do {
            guard let r = try app.envTextShown(id: env.id, text: text) else { return }
            text = r.text
            notShown = r.notShown
            shown = true
            hiding?.cancel()
            hiding = Task {
                try? await Task.sleep(for: Self.shownFor)
                if !Task.isCancelled { hide() }
            }
        } catch {
            problem = "\(error)"
        }
    }

    /// The values shown and not changed out of the text again; what was
    /// typed stays.
    private func hide() {
        hiding?.cancel()
        hiding = nil
        guard shown else { return }
        text = app.envTextHidden(id: env.id, text: text)
        shown = false
        notShown = []
    }
}

// MARK: Notes

/// The notes; in a session (`session`), those about its host first, and
/// a tap opens one to read, its commands to insert.
struct NotesList: View {
    @EnvironmentObject private var app: AppModel
    var session: SessionModel?
    @State private var notes: [NoteRow] = []
    @State private var doomed: Doomed?
    @State private var filter = ""

    var body: some View {
        List {
            if notes.isEmpty {
                Text(tr("No notes yet.")).foregroundStyle(Dusk.muted).listRowBackground(Dusk.surface)
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
                            Text(n.title.isEmpty ? tr("Untitled") : n.title).font(.system(size: 15, weight: .medium))
                            if n.here {
                                Text(tr("this host")).font(.system(size: 11)).foregroundStyle(Dusk.accent)
                            }
                        }
                        Text(detail(n))
                            .font(.system(size: 12))
                            .foregroundStyle(Dusk.muted)
                            .lineLimit(1)
                    }
                }
                .listRowBackground(Dusk.surface)
                .confirming($doomed, about: n.id)
                .swipeActions {
                    if session == nil {
                        DeleteSwipe {
                            doomed = Doomed(
                                key: n.id,
                                title: n.title.isEmpty ? tr("Delete this note?") : tr("Delete {title}?", ["title": n.title]),
                                message: goesEverywhere()
                            ) {
                                app.forget(n.id)
                                reload()
                            }
                        }
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .searchable(text: $filter, prompt: tr("Filter notes"))
        .toolbar {
            if session == nil {
                NavigationLink {
                    NoteEditor(note: NoteRow(id: "", title: "", category: "", markdown: true, sensitive: false, size: 0, hosts: [], here: false))
                } label: { Image(systemName: "plus") }
                .accessibilityLabel(tr("New note"))
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
        var parts = [n.category.isEmpty ? (n.markdown ? tr("Markdown") : tr("Text")) : n.category]
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
    /// The Markdown editor's: its bar's image button, an image put in.
    @StateObject private var editor = MarkdownEditorController()
    /// A table being edited in its grid.
    @State private var tabling: TableSheet?

    var body: some View {
        VStack(spacing: 0) {
            TextField(tr("Title"), text: $title)
                .font(.system(size: 20, weight: .semibold))
                .padding(.horizontal, 16)
                .padding(.top, 10)
            HStack(spacing: 8) {
                TextField(tr("No category"), text: $category)
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
                    Text(tr("Sensitive: its text shows once it is you.")).foregroundStyle(Dusk.muted)
                    Button(tr("Show the text")) { confirmOwner(tr("Show a sensitive note")) { ok in if ok { reveal() } } }
                        .buttonStyle(.bordered)
                }
                .padding(.top, 40)
                Spacer()
            } else if preview && markdown {
                ScrollView {
                    MarkdownView(text: text, toggle: { at in
                        if let ticked = toggleTask(text: text, at: at) { text = ticked }
                    })
                        .padding(16)
                }
            } else if markdown {
                MarkdownEditor(text: $text, controller: editor)
            } else {
                TextEditor(text: $text)
                    .font(.system(size: 15, design: .monospaced))
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
                    Button(preview ? tr("Edit") : tr("Preview")) { preview.toggle() }
                }
            }
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Button(tr("For hosts…"), systemImage: "server.rack") { choosingHosts = true }
                    Toggle(tr("Sensitive"), systemImage: "lock", isOn: $sensitive)
                    Toggle(tr("Markdown"), systemImage: "textformat", isOn: $markdown)
                    if canRead {
                        if markdown {
                            Button(tr("Insert an image…"), systemImage: "photo") { picking = true }
                        }
                        ShareLink(item: exported, preview: SharePreview(title.isEmpty ? tr("Note") : title)) {
                            Label(tr("Export…"), systemImage: "square.and.arrow.up")
                        }
                    }
                    if !id.isEmpty {
                        Button(tr("Delete"), systemImage: "trash", role: .destructive) { confirmDelete = true }
                    }
                } label: { Image(systemName: "ellipsis.circle") }
                .accessibilityLabel(tr("Note"))
                .confirmationDialog(title.isEmpty ? tr("Delete this note?") : tr("Delete {title}?", ["title": title]), isPresented: $confirmDelete, titleVisibility: .visible) {
                    Button(tr("Delete"), role: .destructive) {
                        app.forget(id)
                        deleted = true
                        dismiss()
                    }
                }
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
        .sheet(item: $tabling) { sheet in
            TableEditor(original: sheet.table) { aligns, head, rows in
                editor.setTable(sheet.table, aligns: aligns, head: head, rows: rows)
            }
        }
        .onAppear {
            editor.onImage = { picking = true }
            editor.onTable = { table in tabling = TableSheet(table: table) }
            editor.imageFor = { source in
                source.hasPrefix("attachment:") ? app.noteImage(String(source.dropFirst("attachment:".count))) : nil
            }
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
            problem = tr("That image cannot be read.")
            return
        }
        do {
            guard let line = try app.core?.addNoteImage(bytes: jpeg) else { return }
            if editor.textView != nil && !preview {
                // Where the caret is, on a line of its own.
                editor.insertBlock(line)
            } else {
                text += (text.isEmpty || text.hasSuffix("\n") ? "" : "\n") + line + "\n"
            }
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
        case .failure(let e): app.error = tr("Not saved: {error}", ["error": "\(e)"])
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
            .navigationTitle(tr("For hosts"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button(tr("Done")) { dismiss() } }
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
                Text(note.title.isEmpty ? tr("Untitled") : note.title)
                    .font(.system(size: 22, weight: .semibold))
                if let text {
                    if note.markdown {
                        MarkdownView(
                            text: text,
                            insert: { code in
                                session.paste(code)
                                dismiss()
                            },
                            toggle: { at in tick(at, in: text) }
                        )
                    } else {
                        Text(text).font(.system(size: 14, design: .monospaced)).textSelection(.enabled)
                    }
                } else {
                    Button(tr("Show the text")) {
                        confirmOwner(tr("Show a sensitive note")) { ok in if ok { text = app.noteText(note.id) } }
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

    /// A task ticked here is saved at once: the note is not being edited.
    private func tick(_ at: UInt64, in current: String) {
        guard let ticked = toggleTask(text: current, at: at) else { return }
        text = ticked
        if case .failure(let e) = app.saveNote(
            id: note.id, title: note.title, category: note.category, markdown: note.markdown,
            hosts: note.hosts, sensitive: note.sensitive, text: ticked
        ) {
            app.error = "\(e)"
        }
    }
}
