import Foundation

/// A selectable bypass country for the split-tunnel — an ISO 3166-1 alpha-2
/// code plus a localized display name and flag emoji for the Settings picker.
///
/// The list is intentionally a *curated* short list of the countries users most
/// commonly split-tunnel around, not the full ISO set: any of these resolves to
/// a real network set in the bundled MaxMind GeoLite2 Country mmdb at runtime
/// (`svpn_country_cidrs_file`). A user who needs a code outside the list can
/// still type a custom two-letter code in Settings — ``Profile/bypassCountry``
/// is a free-form string; this catalog only drives the convenience picker.
public struct Country: Identifiable, Hashable, Sendable {
    /// ISO 3166-1 alpha-2 code, uppercase (e.g. `CN`). Also ``id``.
    public let code: String
    /// Localized country name for display.
    public let name: String

    public var id: String { code }

    public init(code: String, name: String) {
        self.code = code
        self.name = name
    }

    /// Regional-indicator flag emoji derived from the two-letter code (e.g.
    /// `CN` → 🇨🇳). Returns an empty string for a non-two-ASCII-letter code.
    public var flag: String {
        let letters = code.uppercased().unicodeScalars.filter { ("A"..."Z").contains($0) }
        guard letters.count == 2 else { return "" }
        var s = ""
        for scalar in letters {
            // Regional Indicator Symbol A == U+1F1E6, 'A' == U+0041.
            if let ri = Unicode.Scalar(0x1F1E6 + (scalar.value - 0x41)) {
                s.unicodeScalars.append(ri)
            }
        }
        return s
    }

    /// `flag name (CODE)`, e.g. `🇨🇳 China (CN)` — the Settings picker row label.
    public var pickerLabel: String {
        let f = flag
        return f.isEmpty ? "\(name) (\(code))" : "\(f) \(name) (\(code))"
    }

    /// Curated catalog, China first (the default), then common split-tunnel
    /// targets in the Asia-Pacific and the rest of the world.
    public static let catalog: [Country] = [
        Country(code: "CN", name: "China"),
        Country(code: "HK", name: "Hong Kong"),
        Country(code: "TW", name: "Taiwan"),
        Country(code: "MO", name: "Macau"),
        Country(code: "JP", name: "Japan"),
        Country(code: "KR", name: "South Korea"),
        Country(code: "SG", name: "Singapore"),
        Country(code: "MY", name: "Malaysia"),
        Country(code: "TH", name: "Thailand"),
        Country(code: "VN", name: "Vietnam"),
        Country(code: "IN", name: "India"),
        Country(code: "ID", name: "Indonesia"),
        Country(code: "PH", name: "Philippines"),
        Country(code: "AU", name: "Australia"),
        Country(code: "RU", name: "Russia"),
        Country(code: "IR", name: "Iran"),
        Country(code: "TR", name: "Turkey"),
        Country(code: "US", name: "United States"),
        Country(code: "GB", name: "United Kingdom"),
        Country(code: "DE", name: "Germany"),
        Country(code: "FR", name: "France"),
        Country(code: "NL", name: "Netherlands"),
        Country(code: "CA", name: "Canada"),
        Country(code: "BR", name: "Brazil"),
    ]

    /// Look up a catalog entry by (case-insensitive) code.
    public static func named(_ code: String) -> Country? {
        let upper = code.uppercased()
        return catalog.first { $0.code == upper }
    }
}
