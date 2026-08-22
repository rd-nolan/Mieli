import SwiftUI

@main
struct MinneApp: App {
    @State private var workspaceManager: WorkspaceManager
    @State private var searchFocus = SearchFocus()
    @State private var saveRequest = SaveRequest()

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
        }
        .commands {
            // T101: a reliable, discoverable shortcut to the search field,
            // independent of the toolbar's implicit ⌘F focus binding.
            CommandMenu("搜索") {
                Button("搜索…") {
                    searchFocus.shouldFocus = true
                }
                .keyboardShortcut("f", modifiers: .command)
            }
            // T102: ⌘S flushes any unsaved editor edit immediately instead of
            // waiting for the 750ms autosave debounce.
            CommandMenu("文件") {
                Button("保存") {
                    saveRequest.fire = true
                }
                .keyboardShortcut("s", modifiers: .command)
            }
        }
    }
}