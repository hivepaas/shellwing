import SwiftUI

/// The host list: the desktop's sidebar as the phone's first screen.
struct HostsView: View {
    @EnvironmentObject private var app: AppModel
    @State private var filter = ""
    @State private var path: [SessionModel] = []
    @State private var editing: HostEdit?
    @State private var editingGroup: GroupSheet?
    @State private var doomed: Doomed?
    @State private var checklist: KeyChecklist?
    @State private var givingKey: CollectionSection?
    @State private var managing: CollectionSection?

    var body: some View {
        NavigationStack(path: $path) {
            List {
                if !app.sessions.isEmpty && filter.isEmpty {
                    Section {
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 8) {
                                ForEach(app.sessions) { s in
                                    OpenSessionChip(model: s) { path = [s] }
                                }
                            }
                            .padding(.horizontal, 16)
                        }
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(Color.clear)
                    } header: {
                        SectionHeading(text: tr("Open"))
                    }
                }
                if !app.checklists.isEmpty && filter.isEmpty {
                    Section {
                        ForEach(app.checklists) { list in
                            Button { checklist = list } label: {
                                HStack(spacing: 12) {
                                    Image(systemName: "exclamationmark.shield")
                                        .foregroundStyle(Dusk.amber)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(tr("Secrets to change in {name}…", ["name": list.name]))
                                            .foregroundStyle(Dusk.text)
                                        Text(tr("Somebody who lost access could read them"))
                                            .font(.system(size: 12))
                                            .foregroundStyle(Dusk.muted)
                                    }
                                    Spacer()
                                    Text("\(list.secrets.count)")
                                        .font(.system(size: 12))
                                        .foregroundStyle(Dusk.muted)
                                }
                            }
                            .listRowBackground(Dusk.surface)
                        }
                    }
                }
                ForEach(shown, id: \.self) { section in
                    Section {
                        if section.hosts.isEmpty {
                            Text(tr("No hosts yet. Choose this group in a host's editor."))
                                .font(.system(size: 14))
                                .foregroundStyle(Dusk.muted)
                                .listRowBackground(Dusk.surface)
                        }
                        ForEach(section.hosts, id: \.id) { host in
                            Button {
                                if let s = app.session(for: host.spec) { path = [s] }
                            } label: {
                                HostRowView(host: host, status: status(of: host))
                            }
                            .listRowBackground(Dusk.surface)
                            .confirming($doomed, about: rowKey(section, host))
                            .swipeActions {
                                DeleteSwipe {
                                    doomed = Doomed(key: rowKey(section, host), title: tr("Delete {title} from hosts.toml?", ["title": host.name])) {
                                        app.delete(hostId: host.id)
                                    }
                                }
                                Button(tr("Edit")) { editing = app.host(id: host.id) }
                                    .tint(Dusk.button)
                            }
                            .contextMenu {
                                Button(tr("Edit"), systemImage: "pencil") { editing = app.host(id: host.id) }
                                Button(tr("Another session"), systemImage: "plus.rectangle.on.rectangle") {
                                    if let s = app.open(host.spec) { path = [s] }
                                }
                            }
                        }
                    } header: {
                        HStack(spacing: 8) {
                            if let c = section.color {
                                RoundedRectangle(cornerRadius: 2).fill(Color(hex: c)).frame(width: 8, height: 8)
                            }
                            if section.favorites {
                                Image(systemName: "star.fill").font(.system(size: 10)).foregroundStyle(Dusk.amber)
                            }
                            SectionHeading(text: section.name ?? tr("Hosts"))
                            if let name = section.name, !section.favorites {
                                Button {
                                    if let g = app.core?.group(name: name) { editingGroup = GroupSheet(edit: g) }
                                } label: {
                                    Image(systemName: "pencil").font(.system(size: 11, weight: .semibold))
                                        .frame(width: 28, height: 22)
                                }
                                .foregroundStyle(Dusk.muted)
                                .accessibilityLabel(tr("Edit {name}", ["name": "\(name)"]))
                            }
                            Spacer()
                            Text(trN(section.hosts.count, "{n} host", "{n} hosts"))
                                .font(.system(size: 12))
                                .foregroundStyle(Dusk.muted)
                        }
                    }
                }
                ForEach(shownCollections, id: \.id) { c in
                    Section {
                        if c.waiting {
                            Text(tr("Waiting for somebody who manages it to give you its key."))
                                .font(.system(size: 14))
                                .foregroundStyle(Dusk.muted)
                                .listRowBackground(Dusk.surface)
                        }
                        ForEach(c.hosts, id: \.id) { host in
                            Button {
                                if let s = app.session(for: host.spec) { path = [s] }
                            } label: {
                                HostRowView(host: host, status: status(of: host))
                            }
                            .listRowBackground(Dusk.surface)
                            .contextMenu {
                                Button(tr("Another session"), systemImage: "plus.rectangle.on.rectangle") {
                                    if let s = app.open(host.spec) { path = [s] }
                                }
                            }
                        }
                    } header: {
                        HStack(spacing: 8) {
                            Image(systemName: "building.2").font(.system(size: 10)).foregroundStyle(Dusk.muted)
                            SectionHeading(text: c.name)
                            if c.permission == "manage" {
                                Button {
                                    managing = c
                                } label: {
                                    Image(systemName: "person.2").font(.system(size: 11, weight: .semibold))
                                        .frame(width: 28, height: 22)
                                }
                                .foregroundStyle(Dusk.muted)
                                .accessibilityLabel(tr("Manage access to {name}…", ["name": c.name]))
                            }
                            if c.permission == "manage" && !c.waiting {
                                Button {
                                    givingKey = c
                                } label: {
                                    Image(systemName: "key").font(.system(size: 11, weight: .semibold))
                                        .frame(width: 28, height: 22)
                                }
                                .foregroundStyle(Dusk.muted)
                                .accessibilityLabel(tr("Give the key of {name}…", ["name": c.name]))
                            }
                            Spacer()
                            Text(collectionNote(c))
                                .font(.system(size: 12))
                                .foregroundStyle(Dusk.muted)
                        }
                    }
                }
                if app.sections.isEmpty && app.collections.isEmpty {
                    empty.listRowBackground(Color.clear)
                }
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .searchable(text: $filter, prompt: tr("Filter by name, tag or address"))
            .navigationTitle("shellwing")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { SyncButton() }
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button(tr("New host"), systemImage: "server.rack") {
                            editing = HostEditorView.blank()
                        }
                        Button(tr("New group"), systemImage: "folder.badge.plus") {
                            editingGroup = GroupSheet(edit: GroupEdit(id: nil, name: "", color: nil, theme: nil))
                        }
                    } label: {
                        Image(systemName: "plus")
                    }
                    .accessibilityLabel(tr("Add a host"))
                }
            }
            .navigationDestination(for: SessionModel.self) { s in
                TerminalScreen(model: s)
            }
            #if DEBUG
            // `-open <host>` at launch (trying it in the Simulator): that
            // session at once.
            .task {
                if path.isEmpty, let spec = UserDefaults.standard.string(forKey: "open"),
                   let s = app.session(for: spec) {
                    path = [s]
                }
                // `-ask delete`: the question for the last host, as its swipe asks.
                if UserDefaults.standard.string(forKey: "ask") == "delete",
                   let section = app.sections.last, let host = section.hosts.last {
                    doomed = Doomed(key: rowKey(section, host), title: tr("Delete {title} from hosts.toml?", ["title": host.name])) {}
                }
                // `-ask newgroup`: a new group's editor.
                if UserDefaults.standard.string(forKey: "ask") == "newgroup" {
                    editingGroup = GroupSheet(edit: GroupEdit(id: nil, name: "", color: nil, theme: nil))
                }
                // `-edit <host>` (or `new`): its editor.
                if let name = UserDefaults.standard.string(forKey: "edit") {
                    let id = app.sections.flatMap(\.hosts).first { $0.name == name }?.id
                    editing = id.flatMap { app.host(id: $0) } ?? HostEditorView.blank()
                }
            }
            #endif
            .sheet(item: $editing) { edit in
                HostEditorView(edit: edit) { saved in
                    editing = nil
                    if let saved, let spec = specOf(id: saved), let s = app.session(for: spec) {
                        path = [s]
                    }
                }
            }
            .sheet(item: $editingGroup) { g in
                GroupEditorView(edit: g.edit) { editingGroup = nil }
            }
            .sheet(item: $checklist) { list in
                KeyChecklistView(list: list) { checklist = nil }
            }
            .sheet(item: $givingKey) { c in
                GiveKeyView(collection: c) { givingKey = nil }
            }
            .sheet(item: $managing) { c in
                AccessView(collection: c) { managing = nil }
            }
            .alert(tr("A collection has a new key"), isPresented: Binding(
                get: { !app.replaced.isEmpty },
                set: { if !$0 { app.replaced = [] } }
            )) {
                Button(tr("OK")) { app.replaced = [] }
            } message: {
                Text(app.replaced.map { name in
                    app.checklists.contains { $0.name == name }
                        ? tr("Somebody lost access to {name}: its key is replaced. Change what they could read: they are listed above your hosts.", ["name": name])
                        : tr("Somebody lost access to {name}: its key is replaced.", ["name": name])
                }.joined(separator: "\n\n"))
            }
            .alert("shellwing", isPresented: Binding(
                get: { app.error != nil },
                set: { if !$0 { app.error = nil } }
            )) {
                Button(tr("OK")) { app.error = nil }
            } message: {
                Text(app.error ?? "")
            }
        }
    }

    private var shown: [HostSection] {
        let q = filter.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return app.sections }
        return app.sections.compactMap { s in
            let hosts = s.hosts.filter {
                $0.name.lowercased().contains(q) || $0.detail.lowercased().contains(q)
                    || $0.tags.contains { $0.lowercased().contains(q) }
            }
            return hosts.isEmpty ? nil : HostSection(name: s.name, color: s.color, hosts: hosts, favorites: s.favorites)
        }
    }

    /// The collections, filtered as the sections are; one waiting for its key
    /// stays while nothing is typed.
    private var shownCollections: [CollectionSection] {
        let q = filter.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return app.collections }
        return app.collections.compactMap { c in
            let hosts = c.hosts.filter {
                $0.name.lowercased().contains(q) || $0.detail.lowercased().contains(q)
                    || $0.tags.contains { $0.lowercased().contains(q) }
            }
            guard !hosts.isEmpty else { return nil }
            var shown = c
            shown.hosts = hosts
            return shown
        }
    }

    /// Beside a collection's name: waiting for its key, read only, or how
    /// many hosts.
    private func collectionNote(_ c: CollectionSection) -> String {
        if c.waiting { return tr("waiting for its key") }
        if c.readOnly { return tr("read only") }
        return trN(c.hosts.count, "{n} host", "{n} hosts")
    }

    /// A row's own: a favorite is listed twice, first and in its group.
    private func rowKey(_ section: HostSection, _ host: HostRow) -> String {
        (section.favorites ? "favorite:" : "") + host.id
    }

    private func status(of host: HostRow) -> SessionStatus? {
        app.sessions.last(where: { $0.spec == host.spec })?.status
    }

    private func specOf(id: String) -> String? {
        app.sections.flatMap(\.hosts).first(where: { $0.id == id })?.spec
    }

    private var empty: some View {
        VStack(spacing: 10) {
            Image(systemName: "server.rack").font(.system(size: 34)).foregroundStyle(Dusk.muted)
            Text(tr("No hosts yet")).font(.headline).foregroundStyle(Dusk.text)
            Text(tr("Add one with +, or sync the hosts of your other devices."))
                .font(.subheadline)
                .foregroundStyle(Dusk.muted)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 40)
    }
}

struct HostRowView: View {
    let host: HostRow
    let status: SessionStatus?

    var body: some View {
        HStack(spacing: 12) {
            let tint = host.color.map { Color(hex: $0) } ?? Dusk.soft
            ZStack(alignment: .bottomTrailing) {
                RoundedRectangle(cornerRadius: 10)
                    .fill(tint.opacity(0.18))
                    .frame(width: 36, height: 36)
                    .overlay(Image(systemName: "server.rack").font(.system(size: 15)).foregroundStyle(tint))
                if let status {
                    Circle()
                        .fill(status.color)
                        .frame(width: 10, height: 10)
                        .overlay(Circle().stroke(Dusk.surface, lineWidth: 2))
                        .offset(x: 2, y: 2)
                }
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(host.name).font(.system(size: 16, weight: .medium)).foregroundStyle(Dusk.text)
                Text(status.map { "\(host.detail) · \($0.said)" } ?? host.detail)
                    .font(.system(size: 13))
                    .foregroundStyle(Dusk.muted)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 4)
    }
}

struct OpenSessionChip: View {
    @ObservedObject var model: SessionModel
    let open: () -> Void

    var body: some View {
        Button(action: open) {
            HStack(spacing: 10) {
                Circle().fill(model.status.color).frame(width: 8, height: 8)
                VStack(alignment: .leading, spacing: 1) {
                    Text(model.spec).font(.system(size: 15, weight: .semibold)).foregroundStyle(Dusk.text)
                    Text(model.title == model.spec ? model.status.said : model.title)
                        .font(.system(size: 12))
                        .foregroundStyle(Dusk.muted)
                        .lineLimit(1)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .frame(minWidth: 150, alignment: .leading)
            .background(Dusk.surface, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Dusk.edge, lineWidth: 1))
        }
    }
}

extension SessionModel: Hashable {
    static func == (a: SessionModel, b: SessionModel) -> Bool { a === b }
    func hash(into h: inout Hasher) { h.combine(ObjectIdentifier(self)) }
}

/// For `.sheet(item:)`: a host is its id; a new one has none.
extension HostEdit: Identifiable {}

/// A group's editor, open.
struct GroupSheet: Identifiable {
    let id = UUID()
    let edit: GroupEdit
}

extension KeyChecklist: Identifiable {
    public var id: String { collection }
}

/// The secrets of a collection whose key this phone replaced, which somebody
/// who lost access could read: to change where they are used, then mark done.
struct KeyChecklistView: View {
    @EnvironmentObject private var app: AppModel
    let list: KeyChecklist
    let done: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(list.secrets, id: \.self) { name in
                        Label(name, systemImage: "key")
                            .foregroundStyle(Dusk.text)
                            .listRowBackground(Dusk.surface)
                    }
                } header: {
                    Text(tr("Somebody who lost access to {name} could read these: change them", ["name": list.name]))
                        .textCase(nil)
                } footer: {
                    Text(tr("Change each where it is used, then here in the collection."))
                }
                Section {
                    Button(tr("All changed: take this list away")) {
                        app.finishChecklist(list.collection)
                        done()
                    }
                    .listRowBackground(Dusk.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .navigationTitle(list.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(tr("Close")) { done() }
                }
            }
        }
    }
}

extension CollectionSection: Identifiable {}

/// Who waits for a collection's key, for somebody who manages it and holds
/// it: each with the fingerprint to compare with their Profile page before
/// giving it.
struct GiveKeyView: View {
    @EnvironmentObject private var app: AppModel
    let collection: CollectionSection
    let done: () -> Void
    @State private var members: [WaitingMember]?
    @State private var problem: String?
    @State private var asking: WaitingMember?
    @State private var giving = false
    @State private var given: String?

    var body: some View {
        NavigationStack {
            List {
                if let problem {
                    Text(problem).foregroundStyle(Dusk.bad).listRowBackground(Dusk.surface)
                }
                if let given {
                    Text(given).foregroundStyle(Dusk.ok).listRowBackground(Dusk.surface)
                }
                if members == nil && problem == nil {
                    ProgressView().frame(maxWidth: .infinity).listRowBackground(Color.clear)
                }
                if let members, members.isEmpty {
                    Text(tr("Everybody with access holds the key."))
                        .foregroundStyle(Dusk.muted)
                        .listRowBackground(Dusk.surface)
                }
                ForEach(members ?? [], id: \.userId) { m in
                    VStack(alignment: .leading, spacing: 6) {
                        Text(m.name).font(.system(size: 16, weight: .medium)).foregroundStyle(Dusk.text)
                        if let f = m.fingerprint, m.served {
                            Text(f)
                                .font(.system(size: 15, design: .monospaced))
                                .foregroundStyle(Dusk.text)
                                .textSelection(.enabled)
                            Text(tr("Fingerprint {fingerprint}: compare it with the one on their Profile page, then give.", ["fingerprint": f]))
                                .font(.system(size: 12))
                                .foregroundStyle(Dusk.muted)
                            Button(tr("Give")) { asking = m }
                                .buttonStyle(.bordered)
                                .disabled(giving)
                        } else if m.fingerprint != nil {
                            Text(tr("Suspended in the organization: they can have it once they are back."))
                                .font(.system(size: 12))
                                .foregroundStyle(Dusk.muted)
                        } else {
                            Text(tr("Has no encryption key yet: they sync the shellwing app with the server once."))
                                .font(.system(size: 12))
                                .foregroundStyle(Dusk.muted)
                        }
                    }
                    .padding(.vertical, 4)
                    .listRowBackground(Dusk.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .navigationTitle(tr("Give the key of {name} to whom?", ["name": collection.name]))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(tr("Close")) { done() }
                }
            }
            .task { await load() }
            .refreshable { await load() }
            .alert(
                tr("Give the key to {name}?", ["name": asking?.name ?? ""]),
                isPresented: Binding(get: { asking != nil }, set: { if !$0 { asking = nil } }),
                presenting: asking
            ) { m in
                Button(tr("Give")) { Task { await give(m) } }
                Button(tr("Cancel"), role: .cancel) {}
            } message: { m in
                Text(tr("Fingerprint {fingerprint}: compare it with the one on their Profile page, then give.", ["fingerprint": m.fingerprint ?? ""]))
            }
        }
    }

    private func load() async {
        guard let core = app.core else { return }
        do {
            members = try await core.waitingForKey(collection: collection.id)
            problem = nil
        } catch {
            problem = tr("Cannot list who waits: {e}", ["e": "\(error)"])
        }
    }

    private func give(_ m: WaitingMember) async {
        guard let core = app.core else { return }
        giving = true
        defer { giving = false }
        do {
            try await core.giveKey(collection: collection.id, userId: m.userId)
            given = tr("{name} has the key of {collection} now.", ["name": m.name, "collection": collection.name])
            problem = nil
            await load()
        } catch {
            problem = tr("The key was not given: {e}", ["e": "\(error)"])
        }
    }
}

extension AccessGrant: Identifiable {
    public var id: String { userId + "|" + groupId }
}

/// A permission's name, and what it lets people do.
private func permissionText(_ p: String) -> (String, String) {
    switch p {
    case "connect": return (tr("Connect"), tr("Connects to its hosts; cannot change them"))
    case "edit": return (tr("Edit"), tr("Changes its hosts and passwords"))
    default: return (tr("Manage"), tr("Edits, grants access and gives the key"))
    }
}

/// Who has access to a collection, for somebody who manages it: each grant's
/// permission changed or taken away, people and groups granted it, then
/// saved at once, as the web app does.
struct AccessView: View {
    @EnvironmentObject private var app: AppModel
    let collection: CollectionSection
    let done: () -> Void
    @State private var access: CollectionAccess?
    @State private var grants: [AccessGrant] = []
    @State private var problem: String?
    @State private var saved: String?
    @State private var saving = false

    private var changed: Bool { access.map { $0.grants != grants } ?? false }

    var body: some View {
        NavigationStack {
            List {
                if let problem {
                    Text(problem).foregroundStyle(Dusk.bad).listRowBackground(Dusk.surface)
                }
                if let saved {
                    Text(saved).foregroundStyle(Dusk.ok).listRowBackground(Dusk.surface)
                }
                if access == nil && problem == nil {
                    ProgressView().frame(maxWidth: .infinity).listRowBackground(Color.clear)
                }
                if let access {
                    Section {
                        ForEach($grants) { $g in
                            HStack(spacing: 10) {
                                Image(systemName: g.groupId.isEmpty ? "person" : "person.3")
                                    .foregroundStyle(Dusk.muted)
                                Text(g.name).foregroundStyle(Dusk.text).lineLimit(1)
                                Spacer()
                                Picker(tr("Permission"), selection: $g.permission) {
                                    ForEach(["connect", "edit", "manage"], id: \.self) { p in
                                        Text(permissionText(p).0).tag(p)
                                    }
                                }
                                .labelsHidden()
                                .pickerStyle(.menu)
                            }
                            .listRowBackground(Dusk.surface)
                            .swipeActions {
                                Button(role: .destructive) {
                                    grants.removeAll { $0.id == g.id }
                                } label: {
                                    Text(tr("Take access away"))
                                }
                            }
                        }
                        Menu {
                            ForEach(access.people.filter { p in !grants.contains { $0.userId == p.userId } }, id: \.userId) { p in
                                Button {
                                    grants.append(AccessGrant(userId: p.userId, groupId: "", name: p.name, permission: "connect"))
                                } label: {
                                    Text(p.name)
                                    if p.owner {
                                        Text(tr("Owner: has Manage already"))
                                    } else if p.suspended {
                                        Text(tr("Suspended: has it once active again"))
                                    }
                                }
                            }
                            ForEach(access.groups.filter { g in !grants.contains { $0.groupId == g.groupId } }, id: \.groupId) { g in
                                Button {
                                    grants.append(AccessGrant(userId: "", groupId: g.groupId, name: g.name, permission: "connect"))
                                } label: {
                                    Text(g.name)
                                    Text(tr("A group: everybody in it"))
                                }
                            }
                        } label: {
                            Label(tr("Grant somebody…"), systemImage: "plus")
                        }
                        .listRowBackground(Dusk.surface)
                    } header: {
                        Text(tr("Who has access to {name}", ["name": collection.name])).textCase(nil)
                    } footer: {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(["connect", "edit", "manage"], id: \.self) { p in
                                Text("\(permissionText(p).0): \(permissionText(p).1)")
                            }
                            Text("\(tr("Take access away")): \(tr("The collection gets a new key, which they never have"))")
                        }
                    }
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .navigationTitle(collection.name)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(changed ? tr("Discard") : tr("Close")) { done() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(tr("Save")) { Task { await save() } }
                        .disabled(!changed || saving)
                }
            }
            .task { await load() }
        }
    }

    private func load() async {
        guard let core = app.core else { return }
        do {
            let a = try await core.collectionAccess(collection: collection.id)
            access = a
            grants = a.grants
            problem = nil
        } catch {
            problem = tr("Cannot read who has access: {e}", ["e": "\(error)"])
        }
    }

    private func save() async {
        guard let core = app.core, let before = access?.grants else { return }
        saving = true
        defer { saving = false }
        do {
            try await core.setAccess(collection: collection.id, grants: grants)
            let added = grants.contains { g in !before.contains { $0.id == g.id } }
            saved = added
                ? tr("Access saved. Anybody newly granted waits for the key: give it from the key button.")
                : tr("Access saved.")
            problem = nil
            await load()
        } catch {
            problem = tr("Access was not changed: {e}", ["e": "\(error)"])
        }
    }
}
