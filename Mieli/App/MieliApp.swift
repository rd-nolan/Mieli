import SwiftUI

@Observable
final class AppLanguage {
    enum Selection: String, CaseIterable, Hashable {
        case system
        case simplifiedChinese = "zh-Hans"
        case english = "en"
    }

    static let preferenceKey = "Mieli.AppLanguage"
    private static let legacyPreferenceKey = "Muisti.AppLanguage"
    private static let historicalPreferenceKey = "Minne.AppLanguage"
    private static let legacyBundleIdentifier = "com.valcub.app.Muisti"
    private static let historicalBundleIdentifier = "com.valcub.app.Minne"

    private let defaults: UserDefaults
    private let preferenceKey: String
    private let preferredLanguages: [String]

    var selection: Selection {
        didSet { defaults.set(selection.rawValue, forKey: preferenceKey) }
    }

    init(
        defaults: UserDefaults = .standard,
        legacyDefaults: UserDefaults? = nil,
        preferenceKey: String = AppLanguage.preferenceKey,
        preferredLanguages: [String] = Locale.preferredLanguages
    ) {
        self.defaults = defaults
        self.preferenceKey = preferenceKey
        self.preferredLanguages = preferredLanguages
        let legacyDefaultsSource = legacyDefaults
            ?? UserDefaults(suiteName: Self.legacyBundleIdentifier)
        let historicalDefaultsSource = UserDefaults(suiteName: Self.historicalBundleIdentifier)
        let storedValue = defaults.string(forKey: preferenceKey)
            ?? (preferenceKey == Self.preferenceKey
                ? legacyDefaultsSource?.string(forKey: Self.legacyPreferenceKey)
                    ?? legacyDefaultsSource?.string(forKey: Self.historicalPreferenceKey)
                    ?? historicalDefaultsSource?.string(forKey: Self.historicalPreferenceKey)
                : nil)
        selection = storedValue
            .flatMap(Selection.init(rawValue:)) ?? .system
        if preferenceKey == Self.preferenceKey,
           defaults.string(forKey: preferenceKey) == nil,
           let storedValue {
            defaults.set(storedValue, forKey: preferenceKey)
        }
    }

    var resolvedIdentifier: String {
        switch selection {
        case .system:
            return preferredLanguages.first?.lowercased().hasPrefix("zh") == true
                ? Selection.simplifiedChinese.rawValue
                : Selection.english.rawValue
        case .simplifiedChinese, .english:
            return selection.rawValue
        }
    }

    var locale: Locale { Locale(identifier: resolvedIdentifier) }

    func isSelected(_ candidate: Selection) -> Bool {
        selection == candidate
    }

    func text(_ key: String) -> String {
        guard resolvedIdentifier == Selection.simplifiedChinese.rawValue,
              let path = Bundle.main.path(
                forResource: Selection.simplifiedChinese.rawValue,
                ofType: "lproj"
              ),
              let bundle = Bundle(path: path) else { return key }
        return bundle.localizedString(forKey: key, value: key, table: "Localizable")
    }

    func format(_ key: String, _ arguments: CVarArg...) -> String {
        String(format: text(key), locale: locale, arguments: arguments)
    }

    func title(for selection: Selection) -> String {
        switch selection {
        case .system: text("Follow System")
        case .simplifiedChinese: text("Chinese (Simplified)")
        case .english: "English"
        }
    }
}

@main
struct MieliApp: App {
    @State private var workspaceManager: WorkspaceManager
    @State private var appLanguage = AppLanguage()
    @State private var searchFocus = SearchFocus()
    @State private var saveRequest = SaveRequest()
    @State private var workspaceSwitch = WorkspaceSwitchRequest()

    init() {
        // Restore a previously persisted workspace, if any. Handles stale
        // bookmarks internally (T012); a successful restore is reflected in
        // the UI via `workspaceURL`.
        let manager = WorkspaceManager()
        manager.restoreWorkspace()
        _workspaceManager = State(initialValue: manager)
    }

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(workspaceManager)
                .environment(searchFocus)
                .environment(saveRequest)
                .environment(workspaceSwitch)
                .environment(appLanguage)
                .environment(\.locale, appLanguage.locale)
        }
        .commands {
            // T101: a reliable, discoverable shortcut to the search field,
            // independent of the toolbar's implicit ⌘F focus binding.
            CommandMenu(appLanguage.text("Search")) {
                Button(appLanguage.text("Search…")) {
                    searchFocus.shouldFocus = true
                }
                .keyboardShortcut("f", modifiers: .command)
            }
            // T111: keep workspace switching in the native File menu instead
            // of creating a duplicate app-owned File menu.
            CommandGroup(after: .newItem) {
                Button(appLanguage.text("Change Workspace…")) {
                    workspaceSwitch.fire = true
                }
                .keyboardShortcut("o", modifiers: [.command, .shift])
            }
            // T102: ⌘S flushes any unsaved editor edit immediately instead of
            // waiting for the 750ms autosave debounce.
            CommandGroup(replacing: .saveItem) {
                Button(appLanguage.text("Save")) {
                    saveRequest.fire = true
                }
                .keyboardShortcut("s", modifiers: .command)
            }
            CommandMenu(appLanguage.text("Language")) {
                ForEach(AppLanguage.Selection.allCases, id: \.self) { selection in
                    Toggle(
                        appLanguage.title(for: selection),
                        isOn: Binding(
                            get: { appLanguage.isSelected(selection) },
                            set: { selected in
                                if selected { appLanguage.selection = selection }
                            }
                        )
                    )
                }
            }
        }
    }
}
