import SVPNModels
import SwiftUI

/// The Profiles tab: manage the list of saved connection profiles. Tap a row's
/// radio to make it the active profile (what Home connects with); tap the row
/// body — or use the toolbar "+" — to edit or create one in a sheet. Swipe to
/// delete (the last profile can't be removed). Switching the active profile
/// while connected takes effect on the next connect.
struct ProfilesView: View {
    @Environment(AppModel.self) private var appModel

    /// The profile currently open in the editor sheet (`nil` when closed).
    /// `Profile` is `Identifiable`, so it drives `.sheet(item:)` directly.
    @State private var editing: Profile?

    var body: some View {
        List {
            Section {
                ForEach(appModel.profiles) { profile in
                    ProfileRow(
                        profile: profile,
                        isActive: profile.id == appModel.selectedProfileID,
                        onSelect: { appModel.selectProfile(profile.id) },
                        onEdit: { editing = profile },
                    )
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        if appModel.profiles.count > 1 {
                            Button(role: .destructive) {
                                appModel.deleteProfile(profile.id)
                            } label: {
                                Label("profiles.delete", systemImage: "trash")
                            }
                            .accessibilityIdentifier("profiles.delete")
                        }
                        Button {
                            editing = profile
                        } label: {
                            Label("profiles.edit", systemImage: "pencil")
                        }
                        .tint(AppTheme.accent)
                    }
                }
            } footer: {
                Text("profiles.section.footer")
            }
        }
        .scrollContentBackground(.hidden)
        .background(AppTheme.screenBackground)
        .navigationTitle("profiles.nav.title")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    editing = appModel.draftProfile()
                } label: {
                    Label("profiles.add", systemImage: "plus")
                }
                .accessibilityIdentifier("profiles.add")
            }
        }
        .sheet(item: $editing) { profile in
            NavigationStack {
                ProfileEditorView(profileID: profile.id, profile: profile)
            }
        }
    }
}

/// One profile row: a radio that sets the active profile, the name + server
/// summary, and a trailing button that opens the editor.
private struct ProfileRow: View {
    let profile: Profile
    let isActive: Bool
    let onSelect: () -> Void
    let onEdit: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Button(action: onSelect) {
                HStack(spacing: 12) {
                    Image(systemName: isActive ? "checkmark.circle.fill" : "circle")
                        .imageScale(.large)
                        .foregroundStyle(isActive ? AppTheme.accent : .secondary)
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(displayName)
                            .font(.body)
                            .foregroundStyle(.primary)
                            .lineLimit(1)
                        serverSummary
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    Spacer(minLength: 8)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("profiles.row.select")
            .accessibilityLabel(Text(displayName))
            .accessibilityValue(isActive ? Text("profiles.row.active") : Text(""))

            Button(action: onEdit) {
                Image(systemName: "slider.horizontal.3")
                    .imageScale(.large)
                    .foregroundStyle(AppTheme.accent)
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("profiles.edit")
            .accessibilityIdentifier("profiles.row.edit")
        }
    }

    private var displayName: String {
        profile.name.isEmpty ? String(localized: "home.profile.none") : profile.name
    }

    private var serverSummary: Text {
        profile.server.isEmpty
            ? Text("profiles.row.noServer")
            : Text(verbatim: "\(profile.serverAddress) · \(profile.mode.displayName)")
    }
}
