import Foundation
import Observation
import os
import SVPNIPC
import SVPNModels

/// Top-level observable that wires the app's long-lived state together and runs
/// first-launch setup. ShadowVPN's model is small: it owns the list of editable
/// ``Profile``s (managed in the Profiles tab), the selected/active one, and the
/// ``VpnManager`` that drives the packet-tunnel extension. There is no SwiftData
/// store, no subscription service and no REST control plane — profiles are plain
/// Codable values persisted in the App Group.
@MainActor
@Observable
final class AppModel {
    /// Every saved connection profile. Always non-empty (the bootstrap migration
    /// seeds at least one). Managed from the Profiles tab; the selected entry is
    /// what Home connects with.
    private(set) var profiles: [Profile]

    /// Identifier of the active profile — the one Home connects with and the
    /// extension reads. Always points at a member of ``profiles``.
    private(set) var selectedProfileID: UUID

    /// Drives the tunnel and publishes connection state + live traffic.
    let vpnManager: VpnManager

    private let log = Logger(subsystem: "com.tangzixiang.shadowvpn.app", category: "app-model")
    private var didBootstrap = false

    init() {
        // Restore the saved profile list. On a clean install (or a build that
        // predates the list) migrate the legacy single profile, or seed a fresh
        // ChinaDNS-ready default — the Profile initializer mirrors the Rust
        // config defaults, so a new profile only needs a server + password.
        let stored = SharedStore.readProfiles()
        if stored.isEmpty {
            let legacy = SharedStore.readProfile() ?? Profile()
            profiles = [legacy]
            selectedProfileID = legacy.id
        } else {
            profiles = stored
            let savedID = SharedStore.readSelectedProfileID()
            selectedProfileID = stored.first(where: { $0.id == savedID })?.id ?? stored[0].id
        }
        vpnManager = VpnManager()
    }

    /// The active profile — the selected entry, with a defensive fallback so the
    /// rest of the app can treat it as non-optional.
    var profile: Profile {
        profiles.first(where: { $0.id == selectedProfileID }) ?? profiles.first ?? Profile()
    }

    /// One-shot async setup. Idempotent — the `.task` modifier can re-invoke it
    /// across scene rebuilds, so the `didBootstrap` guard makes the body run at
    /// most once per process.
    func bootstrap() async {
        guard !didBootstrap else { return }
        didBootstrap = true

        // Mark the persistent profile store as backup-eligible and exclude the
        // transient state/traffic/log files and the per-country CIDR cache
        // (mirrors meow).
        AppGroup.configureBackup()
        // The bypass-CIDR set is derived from the bundled mmdb inside the
        // extension at tunnel start (svpn_country_cidrs_file) and cached per
        // country in the App Group — no app-side staging step is needed.
        // Persist the list + active profile so the extension always has a copy
        // to read at start time even on a brand-new install.
        persist()

        // Load (or install) the NE configuration and seed the initial stage.
        await vpnManager.refresh()
        // Begin watching the shared traffic/state files for live updates.
        vpnManager.startObserving()

        log.notice("bootstrap complete — \(self.profiles.count) profile(s), active=\(self.profile.name, privacy: .public)")
    }

    // MARK: - Profile management

    /// Apply an edited profile: replace it in the list (or append if new),
    /// persist, and — when it's the active one — push it into the live NE
    /// configuration so the next connect uses the new settings. Called from the
    /// profile editor's save path.
    func updateProfile(_ newProfile: Profile) {
        if let idx = profiles.firstIndex(where: { $0.id == newProfile.id }) {
            profiles[idx] = newProfile
        } else {
            profiles.append(newProfile)
        }
        persist()
        if newProfile.id == selectedProfileID {
            Task { await vpnManager.updateConfiguration(with: newProfile) }
        }
    }

    /// A fresh, *uncommitted* profile for the editor to open. It isn't added to
    /// the list until the editor saves it (via ``updateProfile(_:)``, which
    /// appends an id it doesn't yet know), so cancelling leaves nothing behind.
    func draftProfile() -> Profile {
        Profile(name: defaultNewName())
    }

    /// Make `id` the active profile and push its config into the NE so the next
    /// connect uses it. No-op if it's already active or unknown.
    func selectProfile(_ id: UUID) {
        guard id != selectedProfileID, profiles.contains(where: { $0.id == id }) else { return }
        selectedProfileID = id
        persist()
        Task { await vpnManager.updateConfiguration(with: profile) }
    }

    /// Delete a profile. Refuses to remove the last one (the app always needs a
    /// profile). If the active one is removed, the first remaining profile
    /// becomes active and is pushed into the NE.
    func deleteProfile(_ id: UUID) {
        guard profiles.count > 1, profiles.contains(where: { $0.id == id }) else { return }
        let wasActive = id == selectedProfileID
        profiles.removeAll { $0.id == id }
        if wasActive {
            selectedProfileID = profiles[0].id
        }
        persist()
        if wasActive {
            Task { await vpnManager.updateConfiguration(with: profile) }
        }
    }

    /// Persist the list, the selected id, and the active profile copy the
    /// extension reads — always together so the three stay consistent.
    private func persist() {
        do {
            try SharedStore.writeProfiles(profiles)
            try SharedStore.writeProfile(profile)
        } catch {
            log.error("persist profiles failed: \(error.localizedDescription, privacy: .public)")
        }
        SharedStore.writeSelectedProfileID(selectedProfileID)
    }

    /// A non-colliding default name for a newly created profile.
    private func defaultNewName() -> String {
        let base = String(localized: "profiles.new.defaultName")
        let existing = Set(profiles.map(\.name))
        if !existing.contains(base) { return base }
        var n = 2
        while existing.contains("\(base) \(n)") { n += 1 }
        return "\(base) \(n)"
    }

    /// Connect using the current profile. No-op (with a surfaced error) when
    /// the profile is incomplete — Home keeps the toggle disabled in that case,
    /// but guard here too so a programmatic call can't start an empty tunnel.
    func connect() async {
        guard profile.isComplete else {
            vpnManager.clearError()
            log.error("connect blocked — profile incomplete")
            return
        }
        await vpnManager.connect(profile: profile)
    }

    /// Disconnect the tunnel.
    func disconnect() async {
        await vpnManager.disconnect()
    }
}
