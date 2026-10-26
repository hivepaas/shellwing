import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var lock: AppLock

    private let choices = [0, 1, 5, 15, 30, 60]

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Picker("Lock shellwing when away", selection: $lock.after) {
                        ForEach(choices, id: \.self) { m in
                            Text(m == 0 ? "Off" : m == 60 ? "1 hour" : "\(m) min").tag(m)
                        }
                    }
                    .disabled(!lock.available)
                    if lock.after > 0 {
                        Button("Lock now") { lock.lockNow() }
                    }
                } header: {
                    SectionHeading(text: "Lock")
                } footer: {
                    Text(lock.available
                        ? "When shellwing starts, and after this long in the background, it hides until \(lock.biometry) or your passcode says it is you. A light guard for a phone left unlocked: the sessions go on underneath."
                        : "This phone has no passcode, so nothing could ask that it is you.")
                        .foregroundStyle(Dusk.muted)
                }
                .listRowBackground(Dusk.surface)

                Section {
                    LabeledContent("Hosts", value: "\(app.sections.flatMap(\.hosts).count)")
                    LabeledContent("Configuration") {
                        Text("hosts.toml, in the app's own folder")
                            .foregroundStyle(Dusk.muted)
                            .font(.system(size: 14))
                    }
                } header: {
                    SectionHeading(text: "Hosts")
                }
                .listRowBackground(Dusk.surface)

                Section {
                    LabeledContent("Version", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "")
                    Link("shellwing on GitHub", destination: URL(string: "https://github.com/hivepaas/shellwing")!)
                } header: {
                    SectionHeading(text: "About")
                }
                .listRowBackground(Dusk.surface)

                if let problem = app.problem {
                    Section {
                        Text(problem).foregroundStyle(Dusk.badText)
                    } header: {
                        SectionHeading(text: "Problem")
                    }
                    .listRowBackground(Dusk.badBg)
                }
            }
            .scrollContentBackground(.hidden)
            .background(Dusk.panel.ignoresSafeArea())
            .navigationTitle("Settings")
        }
    }
}
