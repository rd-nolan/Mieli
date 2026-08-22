import SwiftUI
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

/// Root view for the Minne macOS app.
///
/// M2 (T021): shows the real Workspace directory tree in a sidebar.
/// The editor, search, and note-creation flows arrive in later tasks.
struct ContentView: View {
    @Environment(WorkspaceManager.self) private var workspace
    @Environment(SearchFocus.self) private var searchFocus
    @Environment(SaveRequest.self) private var saveRequest
    @Environment(WorkspaceSwitchRequest.self) private var workspaceSwitch
    @FocusState private var searchFocused: Bool
    private let logger = Logger(subsystem: "Minne", category: "Editor")
    @State private var showingNewFolder = false
    @State private var newFolderName = ""
    @State private var showingNewNote = false
    @State private var newNoteName = ""
    /// Folder inside which the current new-note/new-folder dialog creates.
    /// `nil` = workspace root (used by the toolbar buttons and list empty area).
    @State private var newItemFolder: String?
    @State private var selectedItem: WorkspaceItem?
    @State private var renamingItem: WorkspaceItem?
    @State private var renameText = ""
    @State private var deletingItem: WorkspaceItem?
    @State private var folderDeleteItem: WorkspaceItem?
    @State private var folderDeleteCount = 0
    @State private var searchText = ""
    @State private var searchResults: [SearchResult] = []
    @State private var searchTask: Task<Void, Never>?
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
                .navigationSplitViewColumnWidth(min: 180, ideal: 220)
                .toolbar {
                    ToolbarItemGroup {
                        Button {
                            beginNewFolder(in: nil)
                        } label: {
                            Label("New Folder", systemImage: "folder.badge.plus")
                        }
                        .help("Create a new folder")
                        .disabled(workspace.workspaceURL == nil)

                        Button {
                            beginNewNote(in: nil)
                        } label: {
                            Label("New Note", systemImage: "square.and.pencil")
                        }
                        .help("Create a new Markdown note")
                        .keyboardShortcut("n", modifiers: .command)
                        .disabled(workspace.workspaceURL == nil)

                        // T111: switch to a different local workspace at any
                        // time — not just on first launch — via the system
                        // folder picker.
                        Button {
                            changeWorkspace()
                        } label: {
                            Label("Change Workspace…", systemImage: "folder")
                        }
                        .help("Choose a different workspace directory")
                        .disabled(workspace.workspaceURL == nil)
                    }
                }
        } detail: {
            if let tag = selectedTag {
                // T074: clicking a sidebar tag lists every note carrying it.
                let notes = taggedNotes
                if notes.isEmpty {
                    Text("没有带标签「\(tag)」的笔记")
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
        .searchable(text: $searchText, placement: .toolbar, prompt: "Search notes")
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
        .onChange(of: searchText) { _, newValue in
            performSearch(newValue)
        }
        // Switching away from a note must persist any unsaved edit rather than
        // drop it (AGENTS §5 — never silently lose user data).
        .onChange(of: selectedItem) { oldValue, _ in
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
        .frame(minWidth: 640, minHeight: 400)
        .overlay { if workspace.workspaceURL == nil { emptyState } }
    }

    var body: some View {
        baseView
        .alert("新建文件夹", isPresented: $showingNewFolder) {
            TextField("文件夹名称", text: $newFolderName)
            Button("创建") {
                if workspace.createFolder(at: scopedNewPath(newFolderName)) {
                    newFolderName = ""
                } else {
                    showError("创建文件夹失败：名称无效或同名文件夹已存在。")
                }
            }
            Button("取消", role: .cancel) {}
        } message: {
            Text("在 Workspace 中创建一个新的真实文件夹。")
        }
        .alert("新建笔记", isPresented: $showingNewNote) {
            TextField("笔记名称", text: $newNoteName)
            Button("创建") {
                let createPath = scopedNewPath(newNoteName)
                if workspace.createNote(at: createPath) {
                    newNoteName = ""
                    // T112: open the note immediately so the user can start
                    // typing instead of hunting for the new row in the tree.
                    openNewNote(createdFrom: createPath)
                } else {
                    showError("创建笔记失败：名称无效或同名笔记已存在。")
                }
            }
            Button("取消", role: .cancel) {}
        } message: {
            Text("在 Workspace 中创建一个新的 Markdown 笔记。")
        }
        .alert(renamingItem?.kind == .note ? "重命名笔记" : "重命名文件夹",
               isPresented: renameAlertBinding) {
            TextField("新名称", text: $renameText)
            Button("重命名") {
                guard let item = renamingItem else { return }
                let succeeded = item.kind == .note
                    ? workspace.renameNote(at: item.relativePath, to: renameText)
                    : workspace.renameFolder(at: item.relativePath, to: renameText)
                if succeeded { renamingItem = nil }
                else { showError("重命名失败：名称无效或同名项目已存在。") }
            }
            Button("取消", role: .cancel) {
                renamingItem = nil
            }
        } message: {
            Text(renamingItem?.kind == .note
                ? "输入新的笔记名称（Markdown）。"
                : "输入新的文件夹名称。")
        }
        .alert("删除笔记", isPresented: deleteAlertBinding) {
            Button("删除", role: .destructive) {
                if let item = deletingItem,
                   workspace.deleteNoteFile(at: item.relativePath) {
                    deletingItem = nil
                    if selectedItem?.relativePath == item.relativePath {
                        selectedItem = nil
                    }
                } else {
                    showError("删除笔记失败。")
                }
            }
            Button("取消", role: .cancel) {
                deletingItem = nil
            }
        } message: {
            Text("确定要永久删除「\(deletingItem?.name ?? "")」吗？此操作不可撤销。")
        }
        .alert(folderDeleteAlertTitle, isPresented: folderDeleteAlertBinding) {
            Button("删除", role: .destructive) {
                if let item = folderDeleteItem,
                   workspace.deleteFolder(at: item.relativePath) {
                    folderDeleteItem = nil
                    if selectedItem?.relativePath == item.relativePath {
                        selectedItem = nil
                    }
                } else {
                    showError("删除文件夹失败。")
                }
            }
            Button("取消", role: .cancel) {
                folderDeleteItem = nil
            }
        } message: {
            Text(folderDeleteMessage)
        }
        .alert("添加标签", isPresented: $showingAddTag) {
            TextField("新标签", text: $addTagText)
            Button("添加") {
                if workspace.addTag(addTagText, toNoteAt: addTagPath) {
                    tagAddRevision += 1
                } else {
                    showError("添加标签失败：无法写入笔记。")
                }
                addTagText = ""
            }
            Button("取消", role: .cancel) { addTagText = "" }
        } message: {
            Text("该标签会写入当前笔记的 Front Matter。留空或重复则不改变。")
        }
        // T095: the open note was changed by another program while this edit is
        // unsaved. Offer an explicit choice — never silently overwrite either side.
        .alert("外部修改笔记", isPresented: $showingConflict) {
            Button("保存我的修改") {
                // Write the local edit over the external change (explicit choice).
                if let path = pendingPath {
                    writePendingMarkdown()
                }
                reloadNote()
                showingConflict = false
            }
            Button("采用外部改动") {
                // Drop the local edit and reload the external version.
                reloadNote()
                showingConflict = false
            }
            Button("取消", role: .cancel) {
                showingConflict = false
            }
        } message: {
            Text("「\(conflictNotePath)」已在另一个程序中修改，而当前编辑尚未保存。请选择处理方式。")
        }
        // T105: a single alert covers important operations that fail (save,
        // create/rename/move/delete, workspace selection). Minimal and
        // deliberate — no notification system, one error at a time.
        .alert("操作失败", isPresented: errorAlertBinding) {
            Button("好") {}
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
                Button {
                    selectedItem = item
                } label: {
                    Label {
                        // T113: hide the `.md` suffix for notes in the tree
                        // while keeping the real filename in `relativePath`/rename.
                        Text(sidebarDisplayName(for: item))
                    } icon: {
                        Image(systemName: iconName(for: item.kind))
                    }
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .listRowBackground(
                    selectedItem?.relativePath == item.relativePath
                        ? Color.accentColor.opacity(0.15)
                        : nil
                )
.contextMenu {
                    Button("Rename…") {
                        renamingItem = item
                        renameText = item.name
                    }
                    if item.kind == .folder {
                        // T103: create inside the right-clicked folder.
                        Divider()
                        Button("New Note…") {
                            beginNewNote(in: item.relativePath)
                        }
                        Button("New Folder…") {
                            beginNewFolder(in: item.relativePath)
                        }
                    }
                    if item.kind == .note {
                        Button("Move to…") {
                            moveToFolder(item)
                        }
                        Divider()
                        Button(role: .destructive) {
                            deletingItem = item
                        } label: {
                            Label("Delete…", systemImage: "trash")
                        }
                    } else if item.kind == .folder {
                        Divider()
                        Button(role: .destructive) {
                            folderDeleteItem = item
                            folderDeleteCount = workspace.folderItemCount(for: item.relativePath) ?? 0
                        } label: {
                            Label("Delete…", systemImage: "trash")
                        }
                    }
                }
            }
            // T103: right-clicking the sidebar's empty area offers the two
            // creation actions at the workspace root (an empty area has no
            // targeted folder). Row-level context menus handle rename/delete.
            .contextMenu {
                Button("New Note…") {
                    beginNewNote(in: nil)
                }
                Button("New Folder…") {
                    beginNewFolder(in: nil)
                }
            }

            // T073: all tags in use, listed under the workspace tree. Selecting
            // a tag is the trigger for filtering notes (T074).
            Divider()
            if sidebarTags.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("标签")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text("暂无标签")
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
            }
        }
        .onAppear { sidebarTags = workspace.allTags() }
        .onChange(of: tagAddRevision) { _, _ in sidebarTags = workspace.allTags() }
        .onChange(of: selectedTag) { _, newTag in
            taggedNotes = newTag.map { workspace.notes(withTag: $0) } ?? []
        }
    }

    private var renameAlertBinding: Binding<Bool> {
        Binding(
            get: { renamingItem != nil },
            set: { if !$0 { renamingItem = nil } }
        )
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
        folderDeleteCount > 0 ? "删除非空文件夹" : "删除文件夹"
    }

    private var folderDeleteMessage: String {
        let name = folderDeleteItem?.name ?? ""
        if folderDeleteCount > 0 {
            return "「\(name)」包含 \(folderDeleteCount) 个项目。删除将永久移除该文件夹及其全部内容，此操作不可撤销。"
        }
        return "确定要永久删除空文件夹「\(name)」吗？此操作不可撤销。"
    }

/// Picks a destination folder in the workspace and moves `item` (a note)
    /// there. Cancellation is a silent no-op; a selection outside the workspace
    /// is rejected by the manager.
    private func moveToFolder(_ item: WorkspaceItem) {
        guard let root = workspace.workspaceURL else { return }
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = true
        panel.directoryURL = root
        panel.prompt = "Move Here"

        guard panel.runModal() == .OK, let url = panel.url else { return }
        guard let relative = workspace.relativeWorkspacePath(of: url) else { return }
        if !workspace.moveNote(at: item.relativePath, toDirectory: relative) {
            // T105: only user-initiated errors. Cancellation returns earlier.
            showError("移动笔记失败：目标文件夹无效或已有同名笔记。")
        }
    }

    /// Opens the "new folder" dialog, scoped to `folder` (nil = workspace root).
    /// T103: used by list empty-area and folder-row context menus, which need a
    /// target directory the toolbar buttons don't have.
    private func beginNewFolder(in folder: String?) {
        newItemFolder = folder
        newFolderName = ""
        showingNewFolder = true
    }

    /// Opens the "new note" dialog, scoped to `folder` (nil = workspace root).
    /// T103: context-menu counterpart of the toolbar's New Note button.
    private func beginNewNote(in folder: String?) {
        newItemFolder = folder
        newNoteName = ""
        showingNewNote = true
    }

    /// Joins `newItemFolder` (when set) and `name` into a workspace-relative
    /// path for `createFolder(at:)` / `createNote(at:)`. A bare name with no
    /// folder context is passed through unchanged (root-level creation).
    private func scopedNewPath(_ name: String) -> String {
        guard let folder = newItemFolder, !folder.isEmpty else { return name }
        return folder + "/" + name
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
        if !workspace.selectWorkspace() {
            showError("无法建立该工作区。请确认目录可访问且可写。")
        }
    }

    private var detailPlaceholder: some View {
        Text("Select a note to edit it.")
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
            Text("这是一个文件夹。选择其中的笔记或在文件夹内新建笔记。")
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
            Text("No Workspace selected")
                .font(.headline)
            Text("Select a local directory to store your notes.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
            Button("Select Workspace…") {
                // T105: surfacing selection failures (bookmark/.minne setup).
                if !workspace.selectWorkspace() {
                    showError("无法建立该工作区。请确认目录可访问且可写。")
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
    private func writePendingMarkdown() {
        guard let md = pendingMarkdown,
              let path = pendingPath,
              let root = workspace.workspaceURL else { return }
        let url = root.appendingPathComponent(path)
        workspace.isSelfWrite = true
        do {
            try FileService.saveMarkdown(md, to: url)
            hasUnsaved = false // this edit now persisted
        } catch {
            logger.error("Save failed for \(path, privacy: .public): \(error.localizedDescription, privacy: .public)")
            workspace.isSelfWrite = false
            // T105: a failed save risks data loss (AGENTS §5) — surface it.
            showError("保存「\(path)」失败：\(error.localizedDescription)\n内容仍保留在编辑器中，请重试。")
            return
        }
        workspace.isSelfWrite = false
        // T066: keep the search index in sync with the saved content.
        workspace.refreshIndex(forNoteAt: path)
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
                        if workspace.removeTag(tag, fromNoteAt: notePath) {
                            tagAddRevision += 1
                        } else {
                            showError("移除标签失败：无法写入笔记。")
                        }
                    } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 8, weight: .semibold))
                    }
                    .buttonStyle(.borderless)
                    .help("移除标签")
                    .accessibilityLabel("移除标签 \(tag)")
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 3)
                .background(.quaternary, in: Capsule())
            }

            if tags.isEmpty {
                Text("无标签")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }

            Button {
                addTagPath = notePath
                addTagText = ""
                showingAddTag = true
            } label: {
                Image(systemName: "plus")
                    .font(.caption)
            }
            .buttonStyle(.borderless)
            .help("添加标签")
            .accessibilityLabel("添加标签")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .frame(maxWidth: .infinity, alignment: .leading)
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
                    Text("没有找到匹配的笔记")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .navigationTitle("Search")
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
        let md = workspace.readNote(at: path) ?? ""
        let noteParentURL = workspace.workspaceURL?
            .appendingPathComponent(path)
            .deletingLastPathComponent()
        VStack(spacing: 0) {
            noteTagsRow(workspace.tags(forNoteAt: path), notePath: path)
                .id(tagAddRevision)
            Divider()
            MarkdownEditorView(
                markdown: md,
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
        else { showError("附件复制失败：无法读取该文件或写入附件目录。") }
    }
}

#Preview {
    ContentView()
        .environment(WorkspaceManager())
}