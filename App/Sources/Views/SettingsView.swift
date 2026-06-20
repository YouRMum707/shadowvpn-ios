import SVPNModels
import SwiftUI

/// App-wide preferences that aren't part of any single connection profile.
/// Connection details (server, cipher, routing, …) now live per-profile in the
/// Profiles tab; Settings keeps the global toggles (on-demand) and the About
/// info. Preferences are persisted straight to the shared `UserDefaults` suite
/// so both the app and the extension agree.
struct SettingsView: View {
    @Environment(VpnManager.self) private var vpnManager

    /// App-wide prefs not part of a connection profile (on-demand, log level).
    @State private var preferences: Preferences = .load(from: AppGroup.defaults)

    var body: some View {
        Form {
            generalSection
            aboutSection
        }
        .scrollContentBackground(.hidden)
        .background(AppTheme.screenBackground)
        .navigationTitle("settings.nav.title")
    }

    // MARK: - General prefs

    private var generalSection: some View {
        Section {
            Toggle("settings.toggle.onDemand", isOn: onDemandBinding)
                .accessibilityIdentifier("settings.toggle.onDemand")
        } header: {
            Text("settings.section.general")
        } footer: {
            Text("settings.section.general.footer")
        }
    }

    private var onDemandBinding: Binding<Bool> {
        Binding(
            get: { preferences.onDemand },
            set: { newValue in
                preferences.onDemand = newValue
                preferences.save(to: AppGroup.defaults)
            },
        )
    }

    // MARK: - About

    private var aboutSection: some View {
        Section("settings.section.about") {
            LabeledContent("settings.about.version", value: appVersion)
                .accessibilityIdentifier("settings.about.version")
            if vpnManager.traffic.footprintMB > 0 {
                LabeledContent("settings.about.memory", value: "\(vpnManager.traffic.footprintMB) MB")
                    .accessibilityIdentifier("settings.about.memory")
                    .accessibilityAddTraits(.updatesFrequently)
            }
        }
    }

    private var appVersion: String {
        (Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String) ?? "1.0.0"
    }
}
