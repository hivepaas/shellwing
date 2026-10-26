import LocalAuthentication
import SwiftUI

/// The SSH keys kept in the vault: made here, their public lines to copy or
/// share, deleted with a swipe.
struct KeysView: View {
    @EnvironmentObject private var app: AppModel
    @State private var keys: [KeyRow] = []
    @State private var naming = false
    @State private var name = "iPhone"
    @State private var copied: String?

    var body: some View {
        List {
            if keys.isEmpty {
                Text(tr("No keys yet. Make one, then install it on a host from a session's menu: it signs in from then on, without a password."))
                    .foregroundStyle(Dusk.muted)
                    .listRowBackground(Dusk.surface)
            }
            ForEach(keys, id: \.id) { k in
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text(k.name).font(.system(size: 16, weight: .medium))
                        Spacer()
                        Text(k.kind)
                            .font(.system(size: 11))
                            .padding(.horizontal, 7)
                            .padding(.vertical, 2)
                            .background(Dusk.chip, in: RoundedRectangle(cornerRadius: 5))
                            .foregroundStyle(Dusk.soft)
                    }
                    Text(k.fingerprint)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(Dusk.muted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    HStack(spacing: 8) {
                        Button(copied == k.id ? tr("Copied") : tr("Copy public key")) {
                            UIPasteboard.general.string = k.public
                            copied = k.id
                        }
                        .buttonStyle(.bordered)
                        ShareLink(item: k.public) { Label(tr("Share"), systemImage: "square.and.arrow.up") }
                            .buttonStyle(.bordered)
                    }
                    .font(.system(size: 13))
                    .padding(.top, 2)
                }
                .padding(.vertical, 4)
                .listRowBackground(Dusk.surface)
                .swipeActions {
                    Button(tr("Delete"), role: .destructive) {
                        app.forget(k.id)
                        keys = app.keys()
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .background(Dusk.panel.ignoresSafeArea())
        .navigationTitle(tr("SSH keys"))
        .toolbar {
            Button { naming = true } label: { Image(systemName: "plus") }
                .accessibilityLabel(tr("Make a key"))
        }
        .alert(tr("A new Ed25519 key"), isPresented: $naming) {
            TextField(tr("Name"), text: $name)
            Button(tr("Make")) {
                _ = app.generateKey(name: name)
                keys = app.keys()
            }
            Button(tr("Cancel"), role: .cancel) {}
        } message: {
            Text(tr("Kept in the vault, encrypted, with your other secrets."))
        }
        .onAppear { keys = app.keys() }
    }
}

/// The passwords and passphrases saved: what they are for, never what they
/// are. A swipe forgets one.
struct SecretsView: View {
    @EnvironmentObject private var app: AppModel
    @State private var secrets: [SavedSecret] = []

    var body: some View {
        List {
            if secrets.isEmpty {
                Text(tr("Nothing saved yet. When a password typed in a session works, shellwing offers to keep it."))
                    .foregroundStyle(Dusk.muted)
                    .listRowBackground(Dusk.surface)
            }
            ForEach(secrets, id: \.id) { s in
                HStack(spacing: 10) {
                    Image(systemName: s.passphrase ? "key" : "person.badge.key")
                        .foregroundStyle(Dusk.soft)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(s.what)
                            .font(.system(size: 15, design: .monospaced))
                            .lineLimit(1)
                            .truncationMode(.middle)
                        Text(s.passphrase ? tr("A key's passphrase") : tr("Password"))
                            .font(.system(size: 12))
                            .foregroundStyle(Dusk.muted)
                    }
                }
                .listRowBackground(Dusk.surface)
                .swipeActions {
                    Button(tr("Forget"), role: .destructive) {
                        app.forget(s.id)
                        secrets = app.savedSecrets()
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .background(Dusk.panel.ignoresSafeArea())
        .navigationTitle(tr("Saved passwords"))
        .onAppear { secrets = app.savedSecrets() }
    }
}

/// The vault's recovery key, shown once Face ID or the passcode says it is
/// the owner: what opens the vault on another device, or here again.
struct RecoveryKeyView: View {
    @EnvironmentObject private var app: AppModel
    @State private var key: String?
    @State private var problem: String?

    var body: some View {
        VStack(spacing: 18) {
            if let key {
                Text(tr("Write it down and keep it apart from this phone. It opens the vault where nothing else can."))
                    .font(.system(size: 15))
                    .foregroundStyle(Dusk.label)
                    .multilineTextAlignment(.center)
                let groups = key.split(separator: "-").map(String.init)
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: 3), spacing: 10) {
                    ForEach(Array(groups.enumerated()), id: \.offset) { _, g in
                        Text(g)
                            .font(.system(size: 18, weight: .medium, design: .monospaced))
                            .frame(maxWidth: .infinity, minHeight: 40)
                            .background(Dusk.field, in: RoundedRectangle(cornerRadius: 8))
                            .overlay(RoundedRectangle(cornerRadius: 8).stroke(Dusk.fieldEdge))
                    }
                }
                Button(tr("Copy")) { UIPasteboard.general.string = key }
                    .buttonStyle(.bordered)
            } else if let problem {
                Text(problem).foregroundStyle(Dusk.badText)
                Button(tr("Try again")) { ask() }
            } else {
                ProgressView()
            }
            Spacer()
        }
        .padding(20)
        .foregroundStyle(Dusk.text)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Dusk.page.ignoresSafeArea())
        .navigationTitle(tr("Recovery key"))
        .onAppear(perform: ask)
    }

    private func ask() {
        problem = nil
        let ctx = LAContext()
        guard ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: nil) else {
            key = app.recoveryKey()
            return
        }
        ctx.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: tr("Show the vault's recovery key")) { ok, _ in
            Task { @MainActor in
                if ok {
                    key = app.recoveryKey()
                    if key == nil { problem = tr("The vault is not open.") }
                } else {
                    problem = tr("Not confirmed: the key stays hidden.")
                }
            }
        }
    }
}
