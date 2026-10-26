import SwiftUI

/// A group: its name, its color, and the theme of its hosts.
struct GroupEditorView: View {
    @EnvironmentObject private var app: AppModel
    let done: () -> Void
    private let oldName: String
    @State private var edit: GroupEdit
    @State private var problem: String?
    @State private var confirmDelete = false

    init(edit: GroupEdit, done: @escaping () -> Void) {
        self.done = done
        oldName = edit.name
        _edit = State(initialValue: edit)
    }

    private var members: Int {
        app.sections.first { !$0.favorites && $0.name == oldName }?.hosts.count ?? 0
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    LabeledContent("Name") {
                        TextField("Production", text: $edit.name)
                    }
                    HStack(spacing: 10) {
                        Text("Color")
                        Spacer()
                        swatch(nil, color: Dusk.button)
                        ForEach(hostColors(), id: \.name) { c in
                            swatch(c.name, color: Color(hex: c.rgb))
                        }
                    }
                    NavigationLink {
                        ThemePicker(selected: edit.theme, appDefault: true) { edit.theme = $0 }
                    } label: {
                        LabeledContent("Theme for its hosts") {
                            Text(themeName).foregroundStyle(Dusk.muted)
                        }
                    }
                } header: {
                    SectionHeading(text: "Group")
                } footer: {
                    Text("A host's own theme wins over its group's. Choose the group in a host's editor to put the host in it.")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)
                if let problem {
                    Text(problem).foregroundStyle(Dusk.badText).listRowBackground(Dusk.badBg)
                }
                Section {
                    Button("Delete…", role: .destructive) { confirmDelete = true }
                        .frame(maxWidth: .infinity)
                }
                .listRowBackground(Dusk.surface)
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.page.ignoresSafeArea())
            .autocorrectionDisabled()
            .navigationTitle(oldName)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel", action: done) }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save", action: save).disabled(edit.name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .confirmationDialog(
                members == 0 ? "Delete \(oldName)?" : "Delete \(oldName)? Its \(members == 1 ? "host stays" : "\(members) hosts stay"), in no group.",
                isPresented: $confirmDelete,
                titleVisibility: .visible
            ) {
                Button("Delete", role: .destructive) {
                    do {
                        try app.core?.deleteGroup(name: oldName)
                        app.reload()
                        done()
                    } catch {
                        problem = "\(error)"
                    }
                }
            }
        }
        .preferredColorScheme(.dark)
    }

    private var themeName: String {
        guard let id = edit.theme else { return "Same as the app" }
        return app.core?.themes().first { $0.id == id }?.name ?? id
    }

    private func swatch(_ name: String?, color: Color) -> some View {
        let on = edit.color == name
        return Button { edit.color = name } label: {
            Circle().fill(color)
                .frame(width: 24, height: 24)
                .overlay(Circle().stroke(on ? Dusk.text : .clear, lineWidth: 2).padding(-3))
                .overlay {
                    if name == nil { Image(systemName: "slash.circle").font(.system(size: 13)).foregroundStyle(Dusk.muted) }
                }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(name ?? "No color")
        .accessibilityAddTraits(on ? .isSelected : [])
    }

    private func save() {
        do {
            _ = try app.core?.saveGroup(edit: edit, oldName: oldName)
            app.reload()
            done()
        } catch {
            problem = "\(error)"
        }
    }
}
