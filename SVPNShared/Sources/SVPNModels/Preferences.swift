import Foundation

// keep in sync with PacketTunnel/Sources/SVPreferences.h SVPrefKey* constants

/// Keys used for the small set of preferences shared via the App Group
/// UserDefaults suite. The active ``Profile`` is persisted separately as JSON;
/// these are the lighter app-wide toggles plus the selected-profile pointer.
public enum PreferenceKey {
    /// JSON-encoded **active** ``Profile`` — the one the extension reads at start
    /// time to build its `config_json`. Kept in lock-step with the selected entry
    /// of ``profileList`` so the NE side needs no multi-profile awareness.
    public static let profile = "com.tangzixiang.shadowvpn.profile"
    /// JSON-encoded array of every saved ``Profile`` the user manages in the
    /// Profiles tab.
    public static let profileList = "com.tangzixiang.shadowvpn.profileList"
    /// Identifier of the selected ``Profile`` (mirrors `Profile.id`).
    public static let selectedProfileID = "com.tangzixiang.shadowvpn.selectedProfileID"
    /// Core log verbosity passed to `svpn_core_log` (0=err..4=trace).
    public static let logLevel = "com.tangzixiang.shadowvpn.logLevel"
    /// Whether the tunnel reconnects on demand when traffic appears.
    public static let onDemand = "com.tangzixiang.shadowvpn.onDemand"
}

public enum PreferenceDefaults {
    public static let logLevel: String = "info"
    public static let onDemand: Bool = false
}

/// App-wide settings that aren't part of a connection ``Profile``. Loaded from
/// and saved to the shared `UserDefaults` suite so both processes agree.
public struct Preferences: Sendable, Equatable {
    public var logLevel: String
    public var onDemand: Bool

    public init(
        logLevel: String = PreferenceDefaults.logLevel,
        onDemand: Bool = PreferenceDefaults.onDemand,
    ) {
        self.logLevel = logLevel
        self.onDemand = onDemand
    }

    public static func load(from defaults: UserDefaults) -> Preferences {
        var prefs = Preferences()
        prefs.logLevel = defaults.string(forKey: PreferenceKey.logLevel) ?? PreferenceDefaults.logLevel
        if defaults.object(forKey: PreferenceKey.onDemand) != nil {
            prefs.onDemand = defaults.bool(forKey: PreferenceKey.onDemand)
        }
        return prefs
    }

    public func save(to defaults: UserDefaults) {
        defaults.set(logLevel, forKey: PreferenceKey.logLevel)
        defaults.set(onDemand, forKey: PreferenceKey.onDemand)
    }
}
