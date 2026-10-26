import SwiftUI

/// A host's name, address, user, port and group. The rest of what
/// `hosts.toml` can say about it (jumps, forwards, a theme) is kept as it is.
struct HostEditorView: View {
    @EnvironmentObject private var app: AppModel
    /// Called with the saved host's id, to connect to; nil when cancelled.
    let done: (String?) -> Void

    @State private var name: String
    @State private var address: String
    @State private var user: String
    @State private var port: String
    @State private var group: String
    @State private var confirmDelete = false
    private let id: String?

    init(edit: HostEdit, done: @escaping (String?) -> Void) {
        self.done = done
        id = edit.id
        _name = State(initialValue: edit.name)
        _address = State(initialValue: edit.address)
        _user = State(initialValue: edit.user ?? "")
        _port = State(initialValue: edit.port.map(String.init) ?? "")
        _group = State(initialValue: edit.group ?? "")
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    LabeledContent("Name") {
                        TextField(address.isEmpty ? "web-1" : address, text: $name)
                    }
                    LabeledContent("Address") {
                        TextField("10.0.1.11 or host.example.com", text: $address)
                            .keyboardType(.URL)
                            .font(.system(.body, design: .monospaced))
                    }
                    LabeledContent("User") {
                        TextField("as the server says", text: $user)
                            .font(.system(.body, design: .monospaced))
                    }
                    LabeledContent("Port") {
                        TextField("22", text: $port)
                            .keyboardType(.numberPad)
                            .font(.system(.body, design: .monospaced))
                    }
                } header: {
                    SectionHeading(text: "Connection")
                } footer: {
                    Text("A password is asked for in the terminal when the server wants one.")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    LabeledContent("Group") {
                        TextField("None", text: $group)
                    }
                    let groups = app.groups()
                    if !groups.isEmpty {
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 6) {
                                ForEach(groups, id: \.self) { g in
                                    Button(g) { group = g }
                                        .font(.system(size: 13, weight: group == g ? .semibold : .regular))
                                        .padding(.horizontal, 10)
                                        .frame(height: 30)
                                        .background(group == g ? Dusk.selected : Dusk.button, in: Capsule())
                                        .overlay(Capsule().stroke(group == g ? Dusk.accent : .clear))
                                        .foregroundStyle(Dusk.text)
                                }
                            }
                        }
                    }
                } header: {
                    SectionHeading(text: "Organize")
                }
                .listRowBackground(Dusk.surface)

                Section {
                    Button("Save and connect") { save(connect: true) }
                        .font(.system(size: 17, weight: .semibold))
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .listRowBackground(Dusk.accent)
                        .foregroundStyle(Dusk.onAccent)
                        .disabled(address.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                if id != nil {
                    Section {
                        Button("Delete…", role: .destructive) { confirmDelete = true }
                            .frame(maxWidth: .infinity)
                    }
                    .listRowBackground(Dusk.surface)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .navigationTitle(id == nil ? "New host" : (name.isEmpty ? address : name))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { done(nil) }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { save(connect: false) }
                        .disabled(address.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .confirmationDialog(
                "Delete \(name.isEmpty ? address : name) from hosts.toml?",
                isPresented: $confirmDelete,
                titleVisibility: .visible
            ) {
                Button("Delete", role: .destructive) {
                    if let id { app.delete(hostId: id) }
                    done(nil)
                }
            }
        }
        .preferredColorScheme(.dark)
    }

    private func save(connect: Bool) {
        let p = UInt16(port.trimmingCharacters(in: .whitespaces))
        let edit = HostEdit(
            id: id,
            name: name,
            address: address,
            port: p,
            user: user.isEmpty ? nil : user,
            group: group.isEmpty ? nil : group
        )
        guard let saved = app.save(edit) else { return }
        done(connect ? saved : nil)
    }
}
