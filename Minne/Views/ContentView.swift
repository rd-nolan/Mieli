import SwiftUI
import AppKit
import OSLog

/// Shared signal that commands (⌘F in the app menu) use to focus the search
/// field and reveal it (T101). The search field lives inside `ContentView`,
/// which the command menu cannot address directly, so a tiny observable
/// carrier is used instead of plumbing focus bindings through the scene.
@MainActor
@Observable
final class SearchFocus {
    /// True while the field should be focused; ContentView consumes this and
    /// clears it after applying focus.
    var shouldFocus = false
}

/// Shared signal that the ⌘S menu command can flush an unsaved edit through.
/// The actual write lives in `ContentView`; the command menu cannot reach it
/// directly, so it trie a one-shot flag the view drains (T102).
@MainActor
@Observable
final class SaveRequest {
    /// True when ⌘S wants immediate persistence; ContentView consumes and
    /// clears it after writing.
    var fire = false
}

/// Shared signal that the "更改工作区…" app menu command (T111) can hand the
/// workspace-switch request to `ContentView`, which owns the pending-edit
/// state that must be persisted before switching. Mirrors `SaveRequest`.
@MainActor
@Observable
final class WorkspaceSwitchRequest {
    /// True when the user asked to switch workspaces; ContentView consumes
    /// and clears it after acting.
    var fire = false
}

/// Applies the compact native scrollbar style to the enclosing SwiftUI List.
/// The probe keeps List semantics intact and only configures its NSScrollView.
struct SidebarScrollerConfigurator: NSViewRepresentable {
    func makeNSView(context: Context) -> ProbeView { ProbeView() }
    func updateNSView(_ nsView: ProbeView, context: Context) { nsView.configureAncestor() }

    static func configure(_ scrollView: NSScrollView) {
        scrollView.scrollerStyle = .overlay
        scrollView.autohidesScrollers = true
        scrollView.verticalScroller?.controlSize = .small
    }

    final class ProbeView: NSView {
        override func viewDidMoveToSuperview() {
            super.viewDidMoveToSuperview()
            configureAncestor()
        }

        func configureAncestor() {
            var ancestor = superview
            while let view = ancestor {
                if let scrollView = view as? NSScrollView {
                    SidebarScrollerConfigurator.configure(scrollView)
                    return
                }
                ancestor = view.superview
            }
        }
    }
}

/// Root view for the Minne macOS app.
///
/// M2 (T021): shows the real Workspace directory tree in a sidebar.
/// The editor, search, and note-creation flows arrive in later tasks.
struct ContentView: View {
    @Environment(WorkspaceManager.self) private var workspace
    @Environment(SearchFocus.self) private var searchFocus
    @Environment(SaveRequest.self) private var saveRequest
    @Environment(WorkspaceSwitchRequest.self) private var workspaceSwitch
    @Environment(AppLanguage.self) private var appLanguage
    @FocusState private var searchFocused: Bool
    @FocusState private var renameFocused: Bool
    @FocusState private var tagInputFocused: Bool
    private let logger = Logger(subsystem: "Minne", category: "Editor")
    @State private var selectedItem: WorkspaceItem?
    @State private var renamingItem: WorkspaceItem?
    @State private var renameText = ""
    @State private var deletingItem: WorkspaceItem?
    @State private var folderDeleteItem: WorkspaceItem?
    @State private var folderDeleteCount = 0
    @State private var searchText = ""
    @State private var searchResults: [SearchResult] = []
    @State private var searchTask: Task<Void, Never>?
    @State private var renameTask: Task<Void, Never>?
    /// True while a debounced search query is in flight; gates the no-results
    /// empty state so typing doesn't flash "no results" before the query
    /// returns (T104).
    @State private var searchInFlight = false
    /// Latest editor content awaiting debounced write (T065).
    @State private var pendingMarkdown: String?
    /// Note path the pending content belongs to (flush on note switch).
    @State private var pendingPath: String?
    @State private var autosaveTask: Task<Void, Never>?
    /// Tag-adding UI state (T071).
    @State private var showingAddTag = false
    @State private var addTagText = ""
    @State private var addTagPath = ""
    @State private var preserveTagInputAfterError = false
    @State private var editorFocusRequest = 0
    /// Bumped after a successful tag add to force the tags row to re-read.
    @State private var tagAddRevision = 0
    /// Tags shown in the sidebar (T073), refreshed when they change.
    @State private var sidebarTags: [String] = []
    @State private var selectedTag: String?
    /// Notes carrying `selectedTag`, loaded when the tag changes (T074).
    @State private var taggedNotes: [TaggedNote] = []
    /// Bumped to force the editor to rebuild (reload latest disk content, T095).
    @State private var editorEpoch = 0
    /// True while there is an unsaved edit (set by editing, cleared on save).
    @State private var hasUnsaved = false
    /// True when an unsaved edit collides with an external change (T095).
    @State private var showingConflict = false
    /// Note path the conflict belongs to.
    @State private var conflictNotePath = ""
    /// Last important error to surface to the user (T105). A single
    /// message-driven alert keeps error presentation minimal — no notification
    /// system — and makes data-loss/operation failures visible.
    @State private var errorMessage: String?
    private var errorAlertBinding: Binding<Bool> {
        Binding(
            get: { errorMessage != nil },
            set: { if !$0 { errorMessage = nil } }
        )
    }

    /// The root navigation shell and its non-modal modifiers. Kept separate
    /// from `body` so the type-checker isn't given one huge expression (the
    /// alert chain alone is large).
    private var baseView: some View {
        NavigationSplitView {
            sidebar
                .navigationSplitViewColumnWidth(min: 280, ideal: 280, max: 400)
                .toolbar {
                    ToolbarItemGroup {
                        Button {
                            beginNewCategory(in: nil)
                        } label: {
                            Label(appLanguage.text("New Category"), systemImage: "folder.badge.plus")
                        }
                        .help(appLanguage.text("New Category"))
                        .disabled(workspace.workspaceURL == nil)

                        Button {
                            beginNewNote(in: nil)
                        } label: {
                            Label(appLanguage.text("New Note"), systemImage: "square.and.pencil")
                        }
                        .help(appLanguage.text("New Note"))
                        .keyboardShortcut("n", modifiers: .command)
                        .disabled(workspace.workspaceURL == nil)

                    }
                }
        } detail: {
            if let tag = selectedTag {
                // T074: clicking a sidebar tag lists every note carrying it.
                let notes = taggedNotes
                if notes.isEmpty {
                    Text(appLanguage.format("No notes tagged “%@”", tag))
                        .foregroundStyle(.secondary)
                } else {
                    List(notes) { note in
                        Button {
                            openTaggedNote(note)
                        } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(note.title)
                                    .lineLimit(1)
                                    .foregroundStyle(.primary)
                                Text(note.folder.isEmpty ? note.filename : "\(note.folder)/\(note.filename)")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
            } else if !searchText.trimmingCharacters(in: .whitespaces).isEmpty {
                searchResultsView
            } else if selectedItem != nil, selectedItem?.kind == .note {
                // T062: load the selected note's Markdown into the editor.
                noteDetail(notePath: selectedItem!.relativePath)
            } else if let item = selectedItem, item.kind == .folder {
                // T104: selecting a folder shows a folder empty state — not a
                // blank editor (the editor is only for notes).
                folderEmptyState(named: item.name)
            } else {
                detailPlaceholder
            }
        }
        .searchable(
            text: $searchText,
            placement: .toolbar,
            prompt: Text(appLanguage.text("Search notes"))
        )
            .searchFocused($searchFocused)
        // T101: the ⌘F menu command requests focus by raising `shouldFocus`;
        // consume it here (and switch to the search field) rather than letting
        // the signal go stale.
        .onChange(of: searchFocus.shouldFocus) { _, newValue in
            guard newValue else { return }
            searchFocused = true
            searchFocus.shouldFocus = false
        }
        // T102: ⌘S flushes any pending edit immediately instead of waiting for
        // the 750ms autosave debounce. A clean no-op when nothing is pending.
        .onChange(of: saveRequest.fire) { _, newValue in
            guard newValue else { return }
            saveRequest.fire = false
            saveNow()
        }
        // T111: the "更改工作区…" menu command triggers the same switch path as
        // the sidebar toolbar button.
        .onChange(of: workspaceSwitch.fire) { _, newValue in
            guard newValue else { return }
            workspaceSwitch.fire = false
            changeWorkspace()
        }
        .onChange(of: workspace.workspaceURL) { _, _ in
            cancelPendingRename()
        }
        .onChange(of: searchText) { _, newValue in
            performSearch(newValue)
        }
        // Switching away from a note must persist any unsaved edit rather than
        // drop it (AGENTS §5 — never silently lose user data).
        .onChange(of: selectedItem) { oldValue, _ in
            if oldValue?.relativePath != selectedItem?.relativePath {
                cancelInlineTagInput()
            }
            guard oldValue?.kind == .note, pendingMarkdown != nil else { return }
            autosaveTask?.cancel()
            autosaveTask = nil
            writePendingMarkdown()
            pendingMarkdown = nil
            pendingPath = nil
        }
        // T095: another program modified/deleted the open note (the manager
        // bumps `externalEventID` and records it in `lastExternalChange`).
        // React here — reload when clean, prompt when there is an unsaved edit,
        // stop editing when the file is gone. Never silently overwrite either side.
        .onChange(of: workspace.externalEventID) { _, _ in
            guard let change = workspace.lastExternalChange else { return }
            handleExternalNoteChange(change.path, isDelete: change.isDelete)
        }
        .onChange(of: errorMessage) { _, newValue in
            guard newValue == nil, preserveTagInputAfterError, showingAddTag else { return }
            preserveTagInputAfterError = false
            Task { @MainActor in
                await Task.yield()
                tagInputFocused = true
            }
        }
        .frame(minWidth: 680, minHeight: 400)
        .overlay { if workspace.workspaceURL == nil { emptyState } }
    }

    var body: some View {
        baseView
        .alert(appLanguage.text("Delete Note"), isPresented: deleteAlertBinding) {
            Button(appLanguage.text("Delete"), role: .destructive) {
                if let item = deletingItem,
                   workspace.deleteNoteFile(at: item.relativePath) {
                    deletingItem = nil
                    if selectedItem?.relativePath == item.relativePath {
                        selectedItem = nil
                    }
                } else {
                    showError(appLanguage.text("Could not delete the note."))
                }
            }
            Button(appLanguage.text("Cancel"), role: .cancel) {
                deletingItem = nil
            }
        } message: {
            Text(appLanguage.format(
                "Permanently delete “%@”? This action cannot be undone.",
                deletingItem?.name ?? ""
            ))
        }
        .alert(folderDeleteAlertTitle, isPresented: folderDeleteAlertBinding) {
            Button(appLanguage.text("Delete"), role: .destructive) {
                if let item = folderDeleteItem,
                   workspace.deleteFolder(at: item.relativePath) {
                    folderDeleteItem = nil
                    if selectedItem?.relativePath == item.relativePath {
                        selectedItem = nil
                    }
                } else {
                    showError(appLanguage.text("Could not delete the folder."))
                }
            }
            Button(appLanguage.text("Cancel"), role: .cancel) {
                folderDeleteItem = nil
            }
        } message: {
            Text(folderDeleteMessage)
        }
        // T095: the open note was changed by another program while this edit is
        // unsaved. Offer an explicit choice — never silently overwrite either side.
        .alert(appLanguage.text("Note Changed Externally"), isPresented: $showingConflict) {
            Button(appLanguage.text("Save My Changes")) {
                // Write the local edit over the external change (explicit choice).
                if let path = pendingPath {
                    writePendingMarkdown()
                }
                reloadNote()
                showingConflict = false
            }
            Button(appLanguage.text("Use External Changes")) {
                // Drop the local edit and reload the external version.
                reloadNote()
                showingConflict = false
            }
            Button(appLanguage.text("Cancel"), role: .cancel) {
                showingConflict = false
            }
        } message: {
            Text(appLanguage.format(
                "“%@” was changed in another app while your edits are unsaved. Choose which version to keep.",
                conflictNotePath
            ))
        }
        // T105: a single alert covers important operations that fail (save,
        // create/rename/move/delete, workspace selection). Minimal and
        // deliberate — no notification system, one error at a time.
        .alert(appLanguage.text("Operation Failed"), isPresented: errorAlertBinding) {
            Button(appLanguage.text("OK")) {}
        } message: {
            Text(errorMessage ?? "")
        }
    }

    private var sidebar: some View {
        VStack(spacing: 0) {
            // Selection is driven explicitly (a row Button sets `selectedItem`)
            // instead of `List(selection:)`. A `List(selection:)` outline also
            // sorts selection conflicts with the adjacent tags list.
            List(workspace.items, children: \.children) { item in
                sidebarRow(for: item)
                .listRowBackground(
                    selectedItem?.relativePath == item.relativePath
                        ? Color.accentColor.opacity(0.15)
                        : nil
                )
.contextMenu {
                    Button(appLanguage.text("Rename")) {
                        startRenaming(item)
                    }
                    if item.kind == .folder {
                        // T103: create inside the right-clicked folder.
                        Divider()
                        Button(appLanguage.text("New Note")) {
                            beginNewNote(in: item.relativePath)
                        }
                        Button(appLanguage.text("New Category")) {
                            beginNewCategory(in: item.relativePath)
                        }
                    }
                    if item.kind == .note {
                        Divider()
                        Button(role: .destructive) {
                            deletingItem = item
                        } label: {
                            Label(appLanguage.text("Delete…"), systemImage: "trash")
                        }
                    } else if item.kind == .folder {
                        Divider()
                        Button(role: .destructive) {
                            folderDeleteItem = item
                            folderDeleteCount = workspace.folderItemCount(for: item.relativePath) ?? 0
                        } label: {
                            Label(appLanguage.text("Delete…"), systemImage: "trash")
                        }
                    }
                }
            }
            // T103: right-clicking the sidebar's empty area offers the two
            // creation actions at the workspace root (an empty area has no
            // targeted folder). Row-level context menus handle rename/delete.
            .contextMenu {
                Button(appLanguage.text("New Note")) {
                    beginNewNote(in: nil)
                }
                Button(appLanguage.text("New Category")) {
                    beginNewCategory(in: nil)
                }
            }
            .background(SidebarScrollerConfigurator())

            // T073: all tags in use, listed under the workspace tree. Selecting
            // a tag is the trigger for filtering notes (T074).
            Divider()
            if sidebarTags.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text(appLanguage.text("Tags"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(appLanguage.text("No Tags"))
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
            } else {
                List(sidebarTags, id: \.self, selection: $selectedTag) { tag in
                    Label(tag, systemImage: "tag")
                        .lineLimit(1)
                }
                .listStyle(.sidebar)
                .background(SidebarScrollerConfigurator())
            }
        }
        .onAppear { sidebarTags = workspace.allTags() }
        .onChange(of: tagAddRevision) { _, _ in sidebarTags = workspace.allTags() }
        .onChange(of: selectedTag) { _, newTag in
            taggedNotes = newTag.map { workspace.notes(withTag: $0) } ?? []
        }
    }

    @ViewBuilder
    private func sidebarRow(for item: WorkspaceItem) -> some View {
        Group {
            if renamingItem?.relativePath == item.relativePath {
                Label {
                    TextField(appLanguage.text("Name"), text: $renameText)
                        .textFieldStyle(.plain)
                        .focused($renameFocused)
                        .onSubmit { commitRename(item) }
                        .onExitCommand { cancelRename() }
                        .onAppear { focusRenameField() }
                        .onChange(of: renameFocused) { wasFocused, isFocused in
                            if wasFocused && !isFocused {
                                commitRename(item)
                            }
                        }
                } icon: {
                    Image(systemName: iconName(for: item.kind))
                }
            } else {
                Button {
                    selectedItem = item
                } label: {
                    Label {
                        // T113: hide the `.md` suffix while the real filename
                        // remains in `relativePath` for filesystem operations.
                        Text(sidebarDisplayName(for: item))
                    } icon: {
                        Image(systemName: iconName(for: item.kind))
                    }
                }
                .buttonStyle(.plain)
            }
        }
        .lineLimit(1)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
    }

    private func startRenaming(_ item: WorkspaceItem) {
        renamingItem = item
        renameText = sidebarDisplayName(for: item)
        focusRenameField()
    }

    private func focusRenameField() {
        DispatchQueue.main.async {
            renameFocused = true
            DispatchQueue.main.async {
                NSApp.sendAction(#selector(NSText.selectAll(_:)), to: nil, from: nil)
            }
        }
    }

    private func cancelRename() {
        renamingItem = nil
        renameFocused = false
    }

    private func commitRename(_ item: WorkspaceItem) {
        guard renamingItem?.relativePath == item.relativePath else { return }
        let trimmed = renameText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            showError(appLanguage.text("Rename failed: the name cannot be empty."))
            focusRenameField()
            return
        }
        guard trimmed != sidebarDisplayName(for: item) else {
            cancelRename()
            if item.kind == .note {
                editorFocusRequest += 1
            }
            return
        }

        let succeeded: Bool
        if item.kind == .note {
            guard prepareForNoteMutation(at: item.relativePath) else { return }
            succeeded = workspace.renameNote(at: item.relativePath, to: trimmed)
            if succeeded {
                selectRenamedNoteIfNeeded(item, newName: trimmed)
            }
        } else {
            succeeded = workspace.renameFolder(at: item.relativePath, to: trimmed)
            if succeeded {
                selectRenamedFolderIfNeeded(item, newName: trimmed)
            }
        }

        if succeeded {
            cancelRename()
            if item.kind == .note {
                editorFocusRequest += 1
            }
        } else {
            showError(appLanguage.text("Rename failed: the name is invalid or already in use."))
            focusRenameField()
        }
    }

    private var deleteAlertBinding: Binding<Bool> {
        Binding(
            get: { deletingItem != nil },
            set: { if !$0 { deletingItem = nil } }
        )
    }

    private var folderDeleteAlertBinding: Binding<Bool> {
        Binding(
            get: { folderDeleteItem != nil },
            set: { if !$0 { folderDeleteItem = nil } }
        )
    }

    private var folderDeleteAlertTitle: String {
        appLanguage.text(folderDeleteCount > 0 ? "Delete Non-Empty Folder" : "Delete Folder")
    }

    private var folderDeleteMessage: String {
        let name = folderDeleteItem?.name ?? ""
        if folderDeleteCount > 0 {
            return appLanguage.format(
                "“%@” contains %lld items. Deleting it permanently removes the folder and all its contents. This action cannot be undone.",
                name,
                Int64(folderDeleteCount)
            )
        }
        return appLanguage.format(
            "Permanently delete the empty folder “%@”? This action cannot be undone.",
            name
        )
    }

    /// Creates a real folder immediately, then selects its default name in the
    /// sidebar so typing replaces it without an intermediate dialog.
    private func beginNewCategory(in folder: String?) {
        guard let path = workspace.createDefaultFolder(in: folder) else {
            showError(appLanguage.text("Could not create the category."))
            return
        }
        let name = (path as NSString).lastPathComponent
        selectedItem = WorkspaceItem(
            name: name,
            kind: .folder,
            relativePath: path,
            children: []
        )
        startRenamingWhenAvailable(at: path)
    }

    /// Creates and opens a Markdown note immediately, then starts inline rename.
    private func beginNewNote(in folder: String?) {
        guard let path = workspace.createDefaultNote(in: folder) else {
            showError(appLanguage.text("Could not create the note."))
            return
        }
        openNewNote(createdFrom: path)
        startRenamingWhenAvailable(at: path)
    }

    /// `WorkspaceManager.refreshTree()` scans off the main actor. Wait for the
    /// new row to arrive before putting its TextField into rename mode.
    private func startRenamingWhenAvailable(at path: String) {
        guard let originWorkspaceURL = workspace.workspaceURL else { return }
        renameTask?.cancel()
        renameTask = Task { @MainActor in
            for attempt in 0..<50 {
                guard !Task.isCancelled,
                      Self.isRenameRequestCurrent(
                        originWorkspaceURL: originWorkspaceURL,
                        currentWorkspaceURL: workspace.workspaceURL
                      ) else { return }
                if let item = selectedWorkspaceItem(for: path) {
                    selectedItem = item
                    renameTask = nil
                    startRenaming(item)
                    return
                }
                if attempt == 10 {
                    workspace.refreshTree()
                }
                do {
                    try await Task.sleep(for: .milliseconds(20))
                } catch {
                    return
                }
            }
            renameTask = nil
            showError(appLanguage.text("The item was created, but the sidebar could not refresh."))
        }
    }

    nonisolated static func isRenameRequestCurrent(
        originWorkspaceURL: URL,
        currentWorkspaceURL: URL?
    ) -> Bool {
        originWorkspaceURL.standardizedFileURL == currentWorkspaceURL?.standardizedFileURL
    }

    private func cancelPendingRename() {
        renameTask?.cancel()
        renameTask = nil
        if renamingItem != nil {
            cancelRename()
        }
    }

    /// T112: selects the just-created note so the editor opens on it. Builds
    /// the Workspace-relative `.md` path exactly as `WorkspaceManager.createNote`
    /// does (append `.md` only when no extension), then selects a matching item
    /// from the (refreshed) tree, falling back to a constructed item so the
    /// editor opens even before the async tree re-scan lands.
    private func openNewNote(createdFrom createPath: String) {
        let basePath = createPath.trimmingCharacters(in: .whitespacesAndNewlines)
        var mdPath = basePath
        if (mdPath as NSString).pathExtension.isEmpty {
            mdPath += ".md"
        }
        // Ensure the folder filter/search don't keep the editor hidden.
        selectedTag = nil
        searchText = ""

        if let item = selectedWorkspaceItem(for: mdPath) {
            selectedItem = item
            return
        }
        let name = (mdPath as NSString).deletingPathExtension
            .components(separatedBy: "/").last ?? mdPath
        selectedItem = WorkspaceItem(name: name, kind: .note, relativePath: mdPath, children: nil)
    }

    /// T111: switch to a different workspace. Persists any unsaved editor edit
    /// (AGENTS §5) and clears workspace-scoped UI state so the new workspace
    /// starts from its own tree/search/selection.
    private func changeWorkspace() {
        // Persist any pending edit before leaving this workspace's tree.
        autosaveTask?.cancel()
        autosaveTask = nil
        if pendingMarkdown != nil {
            writePendingMarkdown()
            pendingMarkdown = nil
            pendingPath = nil
        }
        selectedItem = nil
        selectedTag = nil
        searchText = ""
        if !workspace.selectWorkspace(prompt: appLanguage.text("Select Workspace")) {
            showError(appLanguage.text("Could not open the workspace. Make sure the folder is accessible and writable."))
        }
    }

    private var detailPlaceholder: some View {
        Text(appLanguage.text("Select a note to edit it."))
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// T104: a selected folder has no note content to edit, so show its name
    /// and a hint instead of a blank editor.
    private func folderEmptyState(named name: String) -> some View {
        VStack(spacing: 12) {
            Image(systemName: "folder")
                .font(.system(size: 36))
                .foregroundStyle(.secondary)
            Text("\(name)")
                .font(.headline)
            Text(appLanguage.text("This is a folder. Select a note inside it or create a new note here."))
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var emptyState: some View {
        VStack(spacing: 12) {
            Image(systemName: "folder")
                .font(.system(size: 40))
                .foregroundStyle(.secondary)
            Text(appLanguage.text("No Workspace Selected"))
                .font(.headline)
            Text(appLanguage.text("Select a local folder to store your notes."))
                .font(.subheadline)
                .foregroundStyle(.secondary)
            Button(appLanguage.text("Select Workspace…")) {
                // T105: surfacing selection failures (bookmark/.minne setup).
                if !workspace.selectWorkspace(prompt: appLanguage.text("Select Workspace")) {
                    showError(appLanguage.text("Could not open the workspace. Make sure the folder is accessible and writable."))
                }
            }
            .keyboardShortcut("o", modifiers: .command)
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.regularMaterial)
    }

    /// Debounce window for autosave (AGENTS §17). Pressing keys restart the
    /// timer so a burst of edits produces a single write once typing pauses.
    private static let autosaveDebounce: Duration = .milliseconds(750)

    /// Debounced autosave (T065): remembers the latest Markdown, cancels any
    /// pending write, and persists only after the editor has been quiet for
    /// 750ms. On failure the content is kept so nothing is lost silently.
    private func saveEditorContent(_ md: String) {
        guard let item = selectedItem, item.kind == .note else { return }
        pendingMarkdown = md
        pendingPath = item.relativePath
        // A user edit means there is unsaved content; T095's conflict check
        // branches on this flag, so it must be set here (cleared only once the
        // edit is actually persisted).
        hasUnsaved = true
        autosaveTask?.cancel()
        autosaveTask = Task {
            try? await Task.sleep(for: Self.autosaveDebounce)
            guard !Task.isCancelled else { return }
            writePendingMarkdown()
        }
    }

    /// Writes the latest edited Markdown to the recorded note via the atomic
    /// `FileService` (T064), then refreshes the search index (T066). A failed
    /// write is logged; the editor retains the content.
    @discardableResult
    private func writePendingMarkdown() -> Bool {
        guard let md = pendingMarkdown, let path = pendingPath else { return true }
        guard let root = workspace.workspaceURL else {
            showError(appLanguage.format(
                "Could not save “%@”: the workspace is unavailable.\nYour content is still in the editor. Please try again.",
                path
            ))
            return false
        }
        let url = root.appendingPathComponent(path)
        do {
            try FileService.saveMarkdown(md, to: url)
            // Record the stamp our own write produced so the watcher's delayed
            // `.modified` echo is not mistaken for an external change (T095).
            workspace.recordSelfWrite(at: path)
            hasUnsaved = false // this edit now persisted
        } catch {
            logger.error("Save failed for \(path, privacy: .public): \(error.localizedDescription, privacy: .public)")
            // T105: a failed save risks data loss (AGENTS §5) — surface it.
            showError(appLanguage.format(
                "Could not save “%@”: %@\nYour content is still in the editor. Please try again.",
                path,
                error.localizedDescription
            ))
            return false
        }
        // T066: keep the search index in sync with the saved content.
        workspace.refreshIndex(forNoteAt: path)
        return true
    }

    /// T115: file-backed mutations must not race the editor's debounced save.
    /// Persist the latest Markdown first and keep it in memory if saving fails.
    private func prepareForNoteMutation(at path: String) -> Bool {
        guard pendingPath == path, pendingMarkdown != nil else { return true }
        autosaveTask?.cancel()
        autosaveTask = nil
        guard writePendingMarkdown() else { return false }
        pendingMarkdown = nil
        pendingPath = nil
        return true
    }

    /// T115: replace the selected value's removed path after a successful rename.
    private func selectRenamedNoteIfNeeded(_ item: WorkspaceItem, newName: String) {
        guard selectedItem?.relativePath == item.relativePath else { return }
        let trimmed = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        let filename = trimmed.hasSuffix(".md") ? trimmed : "\(trimmed).md"
        let parent = (item.relativePath as NSString).deletingLastPathComponent
        let newPath = parent.isEmpty ? filename : "\(parent)/\(filename)"
        selectedItem = selectedWorkspaceItem(for: newPath)
            ?? WorkspaceItem(name: filename, kind: .note, relativePath: newPath, children: nil)
        reloadNote()
    }

    private func selectRenamedFolderIfNeeded(_ item: WorkspaceItem, newName: String) {
        guard selectedItem?.relativePath == item.relativePath else { return }
        let trimmed = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        let parent = (item.relativePath as NSString).deletingLastPathComponent
        let newPath = parent.isEmpty ? trimmed : "\(parent)/\(trimmed)"
        selectedItem = selectedWorkspaceItem(for: newPath)
    }

    /// T102: persist any pending edit right away (⌘S). Cancels the pending
    /// autosave debounce so the same content isn't written twice, then writes
    /// immediately. A clean no-op when there is no pending edit.
    private func saveNow() {
        autosaveTask?.cancel()
        autosaveTask = nil
        writePendingMarkdown()
    }

    /// Chip-style row of the current note's Front Matter tags (T070), plus an
    /// "add tag" control (T071).
    private func noteTagsRow(_ tags: [String], notePath: String) -> some View {
        HStack(spacing: 6) {
            ForEach(tags, id: \.self) { tag in
                HStack(spacing: 4) {
                    Text(tag)
                        .font(.caption)
                    Button {
                        guard prepareForNoteMutation(at: notePath) else { return }
                        if workspace.removeTag(tag, fromNoteAt: notePath) {
                            tagAddRevision += 1
                            reloadNote()
                        } else {
                            showError(appLanguage.text("Could not remove the tag because the note could not be written."))
                        }
                    } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 8, weight: .semibold))
                    }
                    .buttonStyle(.borderless)
                    .help(appLanguage.text("Remove Tag"))
                    .accessibilityLabel(appLanguage.format("Remove tag %@", tag))
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 3)
                .background(.quaternary, in: Capsule())
            }

            let isInputActive = showingAddTag && addTagPath == notePath
            if tags.isEmpty && !isInputActive {
                Text(appLanguage.text("No Tags"))
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }

            if showingAddTag, addTagPath == notePath {
                TextField(appLanguage.text("New Tag"), text: $addTagText)
                    .textFieldStyle(.plain)
                    .font(.caption)
                    .focused($tagInputFocused)
                    .onSubmit { commitInlineTagInput(currentTags: tags, notePath: notePath) }
                    .onExitCommand { cancelInlineTagInput() }
                    .onChange(of: tagInputFocused) { wasFocused, isFocused in
                        guard wasFocused, !isFocused, !preserveTagInputAfterError else { return }
                        cancelInlineTagInput()
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .frame(minWidth: 72, idealWidth: 96, maxWidth: 180)
                    .background(.quaternary, in: Capsule())
            }

            Button {
                beginInlineTagInput(notePath: notePath)
            } label: {
                Image(systemName: "plus")
                    .font(.caption)
            }
            .buttonStyle(.borderless)
            .help(appLanguage.text("Add Tag"))
            .accessibilityLabel(appLanguage.text("Add Tag"))
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func beginInlineTagInput(notePath: String) {
        if showingAddTag, addTagPath == notePath {
            tagInputFocused = true
            return
        }
        addTagPath = notePath
        addTagText = ""
        showingAddTag = true
        preserveTagInputAfterError = false
        Task { @MainActor in
            await Task.yield()
            tagInputFocused = true
        }
    }

    private func commitInlineTagInput(currentTags: [String], notePath: String) {
        guard let resolvedTag = NoteTags.resolveTag(
            addTagText,
            currentTags: currentTags,
            workspaceTags: sidebarTags
        ) else {
            cancelInlineTagInput()
            return
        }

        preserveTagInputAfterError = true
        guard prepareForNoteMutation(at: notePath) else { return }
        guard workspace.addTag(resolvedTag, toNoteAt: notePath) else {
            showError(appLanguage.text("Could not add the tag because the note could not be written."))
            return
        }

        preserveTagInputAfterError = false
        if !sidebarTags.contains(where: {
            $0.compare(resolvedTag, options: [.caseInsensitive]) == .orderedSame
        }) {
            sidebarTags.append(resolvedTag)
            sidebarTags.sort { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }
        }
        tagAddRevision += 1
        reloadNote()
        cancelInlineTagInput()
        editorFocusRequest += 1
    }

    private func cancelInlineTagInput() {
        tagInputFocused = false
        showingAddTag = false
        addTagText = ""
        addTagPath = ""
        preserveTagInputAfterError = false
    }

    ///
    /// T095: reacts to an external modification of the open note. If there is
    /// an unsaved edit the user is prompted (never silently discarded); otherwise
    /// a clean reload. A deletion stops editing (file gone on disk).
    private func handleExternalNoteChange(_ path: String, isDelete: Bool) {
        guard let item = selectedItem, item.kind == .note, item.relativePath == path else { return }
        if isDelete {
            autosaveTask?.cancel()
            autosaveTask = nil
            pendingMarkdown = nil
            pendingPath = nil
            hasUnsaved = false
            selectedItem = nil
            return
        }
        if hasUnsaved {
            conflictNotePath = path
            showingConflict = true
        } else {
            reloadNote()
        }
    }

    /// Discards in-memory edited state and forces the editor to rebuild from
    /// the note's disk content (T095).
    private func reloadNote() {
        autosaveTask?.cancel()
        autosaveTask = nil
        pendingMarkdown = nil
        pendingPath = nil
        hasUnsaved = false
        editorEpoch += 1
    }

    private func iconName(for kind: WorkspaceItem.Kind) -> String {
        kind == .folder ? "folder" : "doc"
    }

    /// T113: notes are displayed in the tree without their `.md` extension
    /// (e.g. `技术方案` not `技术方案.md`); folders keep their full name.
    /// The real filename stays in `relativePath` and the rename dialog.
    private func sidebarDisplayName(for item: WorkspaceItem) -> String {
        guard item.kind == .note, item.name.hasSuffix(".md") else {
            return item.name
        }
        return String(item.name.dropLast(3))
    }

    /// T105: surface an important error to the user via the shared alert.
    private func showError(_ message: String) {
        errorMessage = message
    }

    /// Debounced, background search over the workspace index (T057).
    private func performSearch(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        searchTask?.cancel()
        guard !trimmed.isEmpty else {
            searchResults = []
            searchInFlight = false
            return
        }
        guard let queue = workspace.databaseQueue else {
            searchResults = []
            searchInFlight = false
            return
        }
        // Debounce like autosave (AGENTS §17): don't query on every keystroke.
        searchTask = Task { @MainActor in
            searchInFlight = true
            try? await Task.sleep(nanoseconds: 300_000_000)
            guard !Task.isCancelled else { return }
            let results = (try? SearchService.search(trimmed, in: queue)) ?? []
            guard !Task.isCancelled else { return }
            searchResults = results
            searchInFlight = false
        }
    }

    private var searchResultsView: some View {
        Group {
            if searchInFlight || !searchResults.isEmpty {
                // In flight or has results — show the list either way.
                List(searchResults, id: \.id) { result in
                    Button {
                        selectedSearchResult(result)
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(result.title)
                                .font(.headline)
                                .lineLimit(1)
                            Text(result.folder.isEmpty ? result.filename : "\(result.folder)/\(result.filename)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            if let snippet = result.snippet, !snippet.isEmpty {
                                Text(snippet)
                                    .font(.body)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(2)
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            } else {
                // T104: searched, query settled, nothing matched — a hint beats
                // a blank list. (Guarded by `searchInFlight` so typing never
                // flashes a false "no results".)
                VStack(spacing: 8) {
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 32))
                        .foregroundStyle(.secondary)
                    Text(appLanguage.text("No matching notes found"))
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .navigationTitle(appLanguage.text("Search"))
    }

    /// Opens a search result by selecting the matching note in the sidebar.
    ///
    /// Locates the note by its workspace-relative path within the scanned tree
    /// and highlights it. Rendering note content itself belongs to the editor
    /// milestone (T060+); here selection is the "open" action (T058).
    private func selectedWorkspaceItem(for path: String) -> WorkspaceItem? {
        func find(_ items: [WorkspaceItem]) -> WorkspaceItem? {
            for item in items {
                if item.relativePath == path { return item }
                if let children = item.children, let hit = find(children) { return hit }
            }
            return nil
        }
        return find(workspace.items)
    }

    private func selectedSearchResult(_ result: SearchResult) {
        let trimmed = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        if let item = selectedWorkspaceItem(for: result.relativePath) {
            selectedItem = item
            // Clear the search so the sidebar selection stays visible.
            if !trimmed.isEmpty { searchText = "" }
        }
    }

    /// Opens a note picked from the tag-filtered list (T074): select it in the
    /// tree and clear the tag filter so the note editor is shown.
    private func openTaggedNote(_ note: TaggedNote) {
        if let item = selectedWorkspaceItem(for: note.relativePath) {
            selectedItem = item
            selectedTag = nil
        }
    }

    /// The editor pane for a selected note: tags row + the Markdown editor
    /// (T062). Kept out of `body` so the detail's expression stays small
    /// enough for the SwiftUI type-checker.
    @ViewBuilder
    private func noteDetail(notePath path: String) -> some View {
        let md = (pendingPath == path && pendingMarkdown != nil)
            ? pendingMarkdown!
            : (workspace.readNote(at: path) ?? "")
        let noteParentURL = workspace.workspaceURL?
            .appendingPathComponent(path)
            .deletingLastPathComponent()
        VStack(spacing: 0) {
            noteTagsRow(workspace.tags(forNoteAt: path), notePath: path)
                .id(tagAddRevision)
            Divider()
            MarkdownEditorView(
                markdown: md,
                focusRequest: editorFocusRequest,
                languageIdentifier: appLanguage.resolvedIdentifier,
                onContentChanged: { edited in
                    saveEditorContent(edited)
                },
                onAttachmentDropped: { drop, insert in
                    handleAttachmentDrop(drop, notePath: path, insert: insert)
                },
                imageBaseURL: noteParentURL
            )
            .id("\(path)#\(editorEpoch)") // T095: rebuild to reload disk content
        }
        .onAppear { workspace.openedNotePath = path } // T095: open-note tracking
    }

    /// Handles a file dropped into the editor (T083/ T084): copy it into the
    /// note's `.files/` and inject the resulting Markdown fragment.
    private func handleAttachmentDrop(_ drop: MarkdownEditorView.AttachmentDrop,
                                      notePath: String,
                                      insert: @escaping (String) -> Void) {
        let fragment: String? = drop.isImage
            ? workspace.addImageAttachment(from: drop.path, forNoteAt: notePath)
            : workspace.addAttachmentLink(from: drop.path, forNoteAt: notePath)
        if let valid = fragment { insert(valid) }
        else { showError(appLanguage.text("Could not copy the attachment because the file could not be read or the attachment folder could not be written.")) }
    }
}

#Preview {
    ContentView()
        .environment(WorkspaceManager())
        .environment(AppLanguage())
}
