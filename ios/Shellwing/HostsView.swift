import SwiftUI

/// The host list: the desktop's sidebar as the phone's first screen.
struct HostsView: View {
    @EnvironmentObject private var app: AppModel
    @State private var filter = ""
    @State private var path: [SessionModel] = []
    @State private var editing: HostEdit?

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
                        SectionHeading(text: "Open")
                    }
                }
                ForEach(shown, id: \.name) { section in
                    Section {
                        ForEach(section.hosts, id: \.id) { host in
                            Button {
                                if let s = app.session(for: host.spec) { path = [s] }
                            } label: {
                                HostRowView(host: host, status: status(of: host))
                            }
                            .listRowBackground(Dusk.surface)
                            .swipeActions {
                                Button("Delete", role: .destructive) { app.delete(hostId: host.id) }
                                Button("Edit") { editing = app.host(id: host.id) }
                                    .tint(Dusk.button)
                            }
                            .contextMenu {
                                Button("Edit", systemImage: "pencil") { editing = app.host(id: host.id) }
                                Button("Another session", systemImage: "plus.rectangle.on.rectangle") {
                                    if let s = app.open(host.spec) { path = [s] }
                                }
                            }
                        }
                    } header: {
                        HStack(spacing: 8) {
                            if let c = section.color {
                                RoundedRectangle(cornerRadius: 2).fill(Color(hex: c)).frame(width: 8, height: 8)
                            }
                            SectionHeading(text: section.name ?? "Hosts")
                            Spacer()
                            Text(section.hosts.count == 1 ? "1 host" : "\(section.hosts.count) hosts")
                                .font(.system(size: 12))
                                .foregroundStyle(Dusk.muted)
                        }
                    }
                }
                if app.sections.isEmpty {
                    empty.listRowBackground(Color.clear)
                }
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .searchable(text: $filter, prompt: "Filter by name, tag or address")
            .navigationTitle("shellwing")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        editing = HostEdit(id: nil, name: "", address: "", port: nil, user: nil, group: nil)
                    } label: {
                        Image(systemName: "plus")
                    }
                    .accessibilityLabel("Add a host")
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
            .alert("shellwing", isPresented: Binding(
                get: { app.error != nil },
                set: { if !$0 { app.error = nil } }
            )) {
                Button("OK") { app.error = nil }
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
            }
            return hosts.isEmpty ? nil : HostSection(name: s.name, color: s.color, hosts: hosts)
        }
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
            Text("No hosts yet").font(.headline).foregroundStyle(Dusk.text)
            Text("Add one with +, or sync the hosts of your other devices.")
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
