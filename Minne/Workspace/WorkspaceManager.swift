import AppKit
import Foundation
import GRDB
import Observation
import OSLog

/// Owns the user's selected Workspace directory for this session.
///
/// T010: pick a directory and expose its URL.
/// T011: create a Security-Scoped Bookmark for the selection and persist it.
/// T012: restore the bookmark on launch and handle stale bookmarks.
/// T013 (`.minne/` setup) is a separate task.
@Observable
final class WorkspaceManager {
    /// The user-selected Workspace directory, or `nil` before any selection.
    private(set) var workspaceURL: URL?
    private var watcher: WorkspaceWatcher?

    /// Path of the note currently open in the editor (set by the view) —
    /// used to detect external changes to *that* note (T095).
    var openedNotePath: String?

    /// True while the app itself writes a note (atomic save), so the watcher
    /// does not treat it as an *external* change (T095, no self-trigger).
    var isSelfWrite = false

    /// Monotonic counter bumped whenever an external change touches the open
    /// note; the view observes this to react (reload / conflict, T095).
    var externalEventID = 0
    /// The latest external change to the open note (path + whether deleted).
    var lastExternalChange: (path: String, isDelete: Bool)?

    /// The internal `.minne` directory within the workspace, or `nil`
    /// if no workspace is selected.
    var minneDirectoryURL: URL? {
        workspaceURL?.appendingPathComponent(".minne", isDirectory: true)
    }

    /// The rebuildable local search index for the workspace (AGENTS §22).
    ///
    /// Opened once the workspace is active and kept for the session. UI layers
    /// read it via `SearchService` to render results (AGENTS §36). Nil until a
    /// workspace is chosen or restored.
    private(set) var databaseQueue: DatabaseQueue?

    /// The most recent scan of the workspace, as a tree of items.
    private(set) var items: [WorkspaceItem] = []

    /// Re-scans the workspace tree off the main actor and updates `items`.
    ///
    /// No-op with no workspace selected. Failures leave `items` unchanged
    /// (scan errors are surfaced to the user elsewhere).
    func refreshTree() {
        guard let url = workspaceURL else { return }
        let scanned = Task.detached(priority: .userInitiated) {
            try? WorkspaceScanner.scan(url)
        }
        Task { @MainActor in
            if let result = await scanned.value {
                items = result
            }
        }
    }

    /// Opens the workspace search index and performs an incremental reconcile.
    ///
    /// Called after the workspace becomes active (select or restore). Opens
    /// `.minne/index.sqlite` (created on first use) and indexes new / modified
    /// / deleted Markdown off the main actor (AGENTS §33). Failures are logged,
    /// never fatal — a missing/empty index just means search returns nothing.
    /// Starts observing the active workspace for local filesystem changes (T090).
    ///
    /// Watches via FSEvents and logs categorized changes. Index/UI handling of
    /// those changes is wired in later tasks (T091+); T090 establishes the
    /// observation itself. A non-markdown/ignored change is silently dropped
    /// by the watcher. Retained for the app lifetime; nothing to close on
    /// single-workspace shutdown.
    private func startWatching() {
        guard let url = workspaceURL else { return }
        let w = watcher ?? WorkspaceWatcher()
        watcher = w
        w.onChanges = { [weak self] changes in
            guard let self else { return }
            var treeNeedsRefresh = false
            for change in changes {
                switch change.kind {
                case .created:
                    // T091: refresh the sidebar + index for brand-new notes.
                    if change.path.hasSuffix(".md") {
                        self.refreshIndex(forNoteAt: change.path)
                    }
                    treeNeedsRefresh = true
case .modified:
                    // T092: external edits re-index the changed note.
                    if change.path.hasSuffix(".md") {
                        self.refreshIndex(forNoteAt: change.path)
                        self.notifyOpenNoteChanged(change) // T095 (open note)
                    }
                case .deleted:
                    // T093: externally deleted note → drop from index + tree.
                    if change.path.hasSuffix(".md") {
                        self.removeIndex(forNoteAt: change.path)
                        self.notifyOpenNoteChanged(change) // T095 (open note gone)
                    }
                    treeNeedsRefresh = true
                case .renamed:
                    break // T094
                }
            }
            if treeNeedsRefresh { self.refreshTree() }
        }
        w.start(root: url)
    }

    private func openIndex() {
        guard let url = workspaceURL else { return }
        let queue: DatabaseQueue
        do {
            queue = try DatabaseManager.openDatabaseQueue(at: url)
        } catch {
            Logger(subsystem: "Minne", category: "Index")
                .error("openDatabaseQueue failed: \(String(describing: error), privacy: .public)")
            return
        }
        databaseQueue = queue
        Task.detached(priority: .userInitiated) {
            do {
                try IndexUpdater.reconcile(workspace: url, in: queue)
            } catch {
                Logger(subsystem: "Minne", category: "Index")
                    .error("index reconcile failed: \(String(describing: error), privacy: .public)")
            }
        }
    }

    /// Creates a real folder at `relativePath` inside the workspace.
    ///
    /// `relativePath` is Workspace-relative (e.g. `工作/项目B`). Paths that
    /// escape the workspace (`..`), are absolute, empty, or target an internal
    /// directory (`.minne`, `*.files`) are rejected. On success the tree is
    /// re-scanned. Returns `false` when invalid or creation fails.
    func createFolder(at relativePath: String) -> Bool {
        guard let root = workspaceURL else { return false }
        let trimmed = relativePath.trimmingCharacters(in: .whitespacesAndNewlines)
        guard isValidWorkspacePath(trimmed) else { return false }

        let target = root.appendingPathComponent(trimmed)
        // Confine to the workspace root. `root.path` and `target.path` are
        // derived from the same URL and therefore share identical symlink
        // prefixes (`/private/var` etc.), so compare them directly. Only a
        // trailing slash is normalized away.
        var base = root.path
        if base.hasSuffix("/") { base.removeLast() }
        let targetPath = target.path
        guard targetPath == base || targetPath.hasPrefix(base + "/") else { return false }

        do {
            try FileManager.default.createDirectory(
                at: target, withIntermediateDirectories: true)
            refreshTree()
            return true
        } catch {
            return false
        }
    }

    /// Creates a new Markdown note at `name` inside the workspace.
    ///
    /// `name` is the note title (without `.md`) or a Workspace-relative path
    /// (e.g. `工作/周报`). The path is validated like `createFolder`, the
    /// `.md` extension is appended, and an existing file is never overwritten.
    /// Content is minimal Markdown (a title heading); YAML Front Matter is a
    /// later task (T032). On success the tree is re-scanned.
    /// Returns `false` when invalid or creation fails.
    func createNote(at name: String) -> Bool {
        guard let root = workspaceURL else { return false }
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard isValidWorkspacePath(trimmed) else { return false }

        var fileURL = root.appendingPathComponent(trimmed)
        if fileURL.pathExtension.isEmpty {
            fileURL = fileURL.appendingPathExtension("md")
        } else if fileURL.pathExtension != "md" {
            // Reject non-Markdown extensions.
            return false
        }

        // Never overwrite an existing note.
        guard !FileManager.default.fileExists(atPath: fileURL.path) else {
            return false
        }

        let title = fileURL.deletingPathExtension().lastPathComponent
        let content = NoteMetadataFactory.makeNoteContent(title: title) + "\n"

        do {
            // Ensure enclosing folders exist (e.g. 工作/项目A).
            try FileManager.default.createDirectory(
                at: fileURL.deletingLastPathComponent(),
                withIntermediateDirectories: true)
            try content.write(to: fileURL, atomically: true, encoding: .utf8)
            refreshTree()
            return true
        } catch {
            return false
        }
    }

    /// Reads and returns the workspace Markdown note's content as a String.
    ///
    /// Returns `nil` when the path is invalid, is not a `.md` file, or the
    /// file cannot be read (missing, permission error, etc.). Used by the
    /// editor to load a note (T062).
    func readNote(at relativePath: String) -> String? {
        guard let root = workspaceURL else { return nil }
        guard isValidWorkspacePath(relativePath), relativePath.hasSuffix(".md") else {
            return nil
        }
        let fileURL = root.appendingPathComponent(relativePath)
        return try? String(contentsOf: fileURL, encoding: .utf8)
    }

    /// Removes a note from the search index after its file was deleted
    /// (T047 / T093). Off the main actor; missing DB is a no-op; failures log.
    /// Flags that the currently-open note was changed externally (a file
    /// outside the app modified/deleted it), so the view can reload or prompt
    /// for conflict resolution (T095). Suppressed while the app itself is
    /// writing (atomic save → no self-trigger). Never drops User Data: the view
    /// decides whether to reload, keep local edits, or warn.
    private func notifyOpenNoteChanged(_ change: WorkspaceChange) {
        guard !isSelfWrite else { return }
        guard let opened = openedNotePath, opened == change.path else { return }
        lastExternalChange = (change.path, change.kind == .deleted)
        externalEventID += 1
    }

    func removeIndex(forNoteAt relativePath: String) {
        guard let queue = databaseQueue else { return }
        Task.detached(priority: .utility) {
            do {
                try IndexService.remove(relativePath: relativePath, in: queue)
            } catch {
                Logger(subsystem: "Minne", category: "Index")
                    .error("index remove failed for \(relativePath, privacy: .public): \(String(describing: error), privacy: .public)")
            }
        }
    }
    ///
    /// Runs off the main actor so file I/O and indexing never block the UI.
    /// A missing index or DB is a no-op; failures are logged, not fatal.
    func refreshIndex(forNoteAt relativePath: String) {
        guard let queue = databaseQueue, let root = workspaceURL else { return }
        Task.detached(priority: .utility) {
            do {
                try IndexUpdater.updateFile(at: relativePath, workspace: root, in: queue)
            } catch {
                Logger(subsystem: "Minne", category: "Index")
                    .error("index update failed for \(relativePath, privacy: .public): \(String(describing: error), privacy: .public)")
            }
        }
    }

    /// Returns the Front Matter `tags` of the note at `relativePath` (T070).
    ///
    /// Missing note or absent Front Matter yields an empty array.
    func tags(forNoteAt relativePath: String) -> [String] {
        guard let md = readNote(at: relativePath) else { return [] }
        return FrontMatterParser.parse(md)?.tags ?? []
    }

    /// Appends `tag` to a note's Front Matter tags (T071) and refreshes its index.
    ///
    /// Persists via the atomic `FileService`, then updates the search index
    /// (`refreshIndex`). Returns `false` when the note is missing or the write
    /// fails; a repeated/blank tag is an idempotent no-op that returns `true`.
    func addTag(_ tag: String, toNoteAt relativePath: String) -> Bool {
        guard let root = workspaceURL else { return false }
        guard let md = readNote(at: relativePath) else { return false }
        let updated = NoteTags.addTag(tag, to: md)
        guard updated != md else { return true } // no change (already tagged / blank)

        let url = root.appendingPathComponent(relativePath)
        do {
            try FileService.saveMarkdown(updated, to: url)
        } catch {
            return false
        }
        refreshIndex(forNoteAt: relativePath)
        return true
    }

    /// Removes `tag` from a note's Front Matter tags (T072) and refreshes index.
    ///
    /// Persists via the atomic `FileService` then refreshes the search index.
    /// Returns `false` on a missing note or write failure; a tag that isn't
    /// present (or blank) is an idempotent no-op that returns `true`.
    func removeTag(_ tag: String, fromNoteAt relativePath: String) -> Bool {
        guard let root = workspaceURL else { return false }
        guard let md = readNote(at: relativePath) else { return false }
        let updated = NoteTags.removeTag(tag, from: md)
        guard updated != md else { return true } // no change (tag not present)

        let url = root.appendingPathComponent(relativePath)
        do {
            try FileService.saveMarkdown(updated, to: url)
        } catch {
            return false
        }
        refreshIndex(forNoteAt: relativePath)
        return true
    }

    /// Returns every tag currently used by indexed notes, sorted by name (T073).
    /// Empty when there is no index/database.
    func allTags() -> [String] {
        guard let queue = databaseQueue else { return [] }
        let sql = """
            SELECT DISTINCT name FROM tags
            WHERE id IN (SELECT tag_id FROM note_tags)
            ORDER BY name COLLATE NOCASE
            """
        return (try? queue.read { db in try String.fetchAll(db, sql: sql) }) ?? []
    }

    /// Returns every note carrying `tag`, sorted by filename (T074). Empty when
    /// there is no index or the tag is not in use.
    func notes(withTag tag: String) -> [TaggedNote] {
        guard let queue = databaseQueue else { return [] }
        return (try? IndexService.taggedNotes(tag: tag, in: queue)) ?? []
    }

    /// Atomically stores `sourcePath` into a note's `.files/` folder and
    /// returns the Markdown image fragment to insert (T083).
    ///
    /// The returned fragment is relative to the note's parent directory,
    /// e.g. `![x.png](./技术方案.files/x-1.png)` (AGENTS §18). Returns `nil`
    /// when the source is missing or the copy fails.
    func addImageAttachment(from sourcePath: String, forNoteAt relativePath: String) -> String? {
        guard let root = workspaceURL, !sourcePath.isEmpty else { return nil }
        let source = URL(fileURLWithPath: sourcePath)
        guard let dest = try? AttachmentService.copyAttachmentUnique(
            from: source, forNoteRelativePath: relativePath, in: root) else { return nil }
        let filename = (relativePath as NSString).lastPathComponent
        let stem = (filename as NSString).deletingPathExtension
        let folderName = stem + ".files"
        return "![\(dest.lastPathComponent)](./\(folderName)/\(dest.lastPathComponent))"
    }

    /// Copies a non-image file into a note's `.files/` folder and returns the
    /// Markdown *link* fragment to insert (T084): `[x.pdf](./技术方案.files/x.pdf)`.
    /// Returns `nil` when the source is missing or the copy fails.
    func addAttachmentLink(from sourcePath: String, forNoteAt relativePath: String) -> String? {
        guard let root = workspaceURL, !sourcePath.isEmpty else { return nil }
        let source = URL(fileURLWithPath: sourcePath)
        guard let dest = try? AttachmentService.copyAttachmentUnique(
            from: source, forNoteRelativePath: relativePath, in: root) else { return nil }
        let filename = (relativePath as NSString).lastPathComponent
        let stem = (filename as NSString).deletingPathExtension
        let folderName = stem + ".files"
        return "[\(dest.lastPathComponent)](./\(folderName)/\(dest.lastPathComponent))"
    }

    /// Renames a real folder at `relativePath` to `newName` in the same parent.
    ///
    /// `newName` is a single folder name (no path separators). It must be
    /// non-empty, not `.`/`..`, and not an internal name (`.minne`, `*.files`).
    /// The destination folder must not already exist (no silent overwrite). On
    /// success the tree is re-scanned. Returns `false` when invalid or the
    /// move fails.
    func renameFolder(at relativePath: String, to newName: String) -> Bool {
        guard let root = workspaceURL else { return false }
        guard isValidWorkspacePath(relativePath) else { return false }
        let trimmedName = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard isValidFolderName(trimmedName) else { return false }

        let source = root.appendingPathComponent(relativePath)
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(
            atPath: source.path, isDirectory: &isDir), isDir.boolValue else {
            return false
        }

        let destination = source.deletingLastPathComponent()
            .appendingPathComponent(trimmedName)
        // Never silently overwrite an existing folder.
        guard !FileManager.default.fileExists(atPath: destination.path) else {
            return false
        }

        do {
            try FileManager.default.moveItem(at: source, to: destination)
            refreshTree()
            return true
        } catch {
            return false
        }
    }

    /// Renames a Markdown note at `relativePath` to a new name in the same parent.
    ///
    /// `newName` is the new title, with or without the `.md` extension; a
    /// non-Markdown extension is rejected. The source must be an existing
    /// `.md` file, and the destination must not already exist (no silent
    /// overwrite). Only the Markdown file is renamed — a sibling `*.files`
    /// attachment directory is handled by a later task (T086). On success the
    /// tree is re-scanned. Returns `false` when invalid or the move fails.
    func renameNoteFile(at relativePath: String, to newName: String) -> Bool {
        guard let root = workspaceURL else { return false }
        guard isValidWorkspacePath(relativePath) else { return false }

        let trimmedName = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedName.isEmpty, !trimmedName.contains("/"),
              trimmedName != "." && trimmedName != ".." else { return false }

        // Normalize the target name to the note's own `.md`.
        var targetName = trimmedName
        if targetName.hasSuffix(".md") {
            // keep
        } else if targetName.contains(".") {
            // Reject non-Markdown extensions (e.g. `.jpg`).
            return false
        } else {
            targetName += ".md"
        }

        let source = root.appendingPathComponent(relativePath)
        var isDir: ObjCBool = false
        // The source must be an actual file (`.md`), never a folder.
        guard FileManager.default.fileExists(
            atPath: source.path, isDirectory: &isDir), !isDir.boolValue else {
            return false
        }

        let destination = source.deletingLastPathComponent()
            .appendingPathComponent(targetName)
        // Never silently overwrite an existing note.
        guard !FileManager.default.fileExists(atPath: destination.path) else {
            return false
        }

        do {
            try FileManager.default.moveItem(at: source, to: destination)
            refreshTree()
            return true
        } catch {
            return false
        }
    }

    /// Renames a note including its attachments and relative links (T086).
    ///
    /// Renames the `.md` file, moves its sibling `<old>.files/` folder to
    /// `<new>.files/`, and rewrites `./<old>.files/…` paths inside the note's
    /// markdown. Attachment handling is best-effort: if the new attachment
    /// folder would overwrite an existing one (or the move fails), the note is
    /// still renamed but keeps its old folder + paths intact — never merged or
    /// deleted (AGENTS §5 data safety).
    func renameNote(at relativePath: String, to newName: String) -> Bool {
        guard let root = workspaceURL, isValidWorkspacePath(relativePath) else { return false }
        guard renameNoteFile(at: relativePath, to: newName) else { return false }

        let trimmed = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        var targetName = trimmed
        if !targetName.hasSuffix(".md") { targetName += ".md" }

        let oldLeaf = (relativePath as NSString).lastPathComponent
        let oldStem = oldLeaf.hasSuffix(".md")
            ? String(oldLeaf.dropLast(3)) : oldLeaf
        let newStem = (targetName as NSString).deletingPathExtension
        guard oldStem != newStem else { return true }

        // Move the attachment folder (skip quietly when renamed in place).
        // The folder lives beside the note, so base it on the note's parent.
        let noteParent = (relativePath as NSString).deletingLastPathComponent
        let attachRoot = noteParent.isEmpty
            ? root : root.appendingPathComponent(noteParent)
        let moved = AttachmentService.renameAttachmentFolder(
            fromNoteStem: oldStem, toNoteStem: newStem, in: attachRoot)
        guard moved else { return true } // keep old folder + links (no data loss)

        // Rewrite relative attachment links in the note body.
        let newRel = noteParent.isEmpty ? targetName : "\(noteParent)/\(targetName)"
        guard let md = readNote(at: newRel) else { return true }
        let rewritten = AttachmentService.rewritingAttachmentLinks(
            md, oldStem: oldStem, newStem: newStem)
        if rewritten != md {
            let url = root.appendingPathComponent(newRel)
            _ = try? FileService.saveMarkdown(rewritten, to: url)
            refreshIndex(forNoteAt: newRel)
        }
        return true
    }

    /// Moves a Markdown note at `relativePath` into the workspace-relative
    /// folder `toDirectory` (empty means the workspace root).
    ///
    /// The source must be an existing `.md` file and the target an existing
    /// directory (never `.minne`/`*.files`, which `isValidWorkspacePath`
    /// rejects). An existing file at the destination is never overwritten.
    /// On success the tree is re-scanned. Returns `false` on any invalid or
    /// failed move.
    func moveNote(at relativePath: String, toDirectory directory: String) -> Bool {
        guard let root = workspaceURL else { return false }
        // Source: an existing Markdown file.
        guard isValidWorkspacePath(relativePath), relativePath.hasSuffix(".md") else {
            return false
        }
        let source = root.appendingPathComponent(relativePath)
        var sourceIsDir: ObjCBool = false
        guard FileManager.default.fileExists(
            atPath: source.path, isDirectory: &sourceIsDir), !sourceIsDir.boolValue else {
            return false
        }

        // Target: an existing directory (empty = workspace root).
        let trimmedDir = directory.trimmingCharacters(in: .whitespacesAndNewlines)
        let targetDir: URL
        if trimmedDir.isEmpty {
            targetDir = root
        } else {
            guard isValidWorkspacePath(trimmedDir) else { return false }
            targetDir = root.appendingPathComponent(trimmedDir)
            var isDir: ObjCBool = false
            guard FileManager.default.fileExists(
                atPath: targetDir.path, isDirectory: &isDir), isDir.boolValue else {
                return false
            }
        }

        let destination = targetDir.appendingPathComponent(source.lastPathComponent)
        // Never silently overwrite an existing note (also rejects moving a note
        // into the directory it already lives in).
        guard !FileManager.default.fileExists(atPath: destination.path) else {
            return false
        }

        do {
            try FileManager.default.moveItem(at: source, to: destination)
            refreshTree()
            return true
        } catch {
            return false
        }
    }

    /// Converts an absolute URL inside the workspace to a workspace-relative
    /// directory string (`""` for the workspace root), or `nil` if `url` is
    /// outside the workspace. Resolves symlinks on both sides so the
    /// `/private` prefix does not cause false mismatches.
    func relativeWorkspacePath(of url: URL) -> String? {
        guard let root = workspaceURL else { return nil }
        let rootResolved = root.resolvingSymlinksInPath().path
        let target = url.resolvingSymlinksInPath().path
        if target == rootResolved { return "" }
guard target.hasPrefix(rootResolved + "/") else { return nil }
        return String(target.dropFirst(rootResolved.count + 1))
    }

    /// Permanently deletes the Markdown note at `relativePath`.
    ///
    /// Only an existing real `.md` file (never a folder) is removed. There is
    /// no Trash System (per T027) — this is a permanent delete, so callers must
    /// confirm with the user first. The sibling `*.files` attachment directory
    /// is not touched by this task. On success the tree is re-scanned.
    /// Returns `false` when the path is invalid or deletion fails.
    func deleteNoteFile(at relativePath: String) -> Bool {
        guard let root = workspaceURL else { return false }
        guard isValidWorkspacePath(relativePath), relativePath.hasSuffix(".md") else {
            return false
        }
        let file = root.appendingPathComponent(relativePath)
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(
            atPath: file.path, isDirectory: &isDir), !isDir.boolValue else {
            return false
        }
        do {
            try FileManager.default.removeItem(at: file)
            refreshTree()
            return true
        } catch {
            return false
        }
    }

    /// Counts the items (files and folders, recursively) directly under the
    /// folder at `relativePath`. Returns `nil` if the path is not an existing
    /// directory. Used by the UI to warn before deleting a non-empty folder.
    func folderItemCount(for relativePath: String) -> Int? {
        guard let root = workspaceURL else { return nil }
        guard isValidWorkspacePath(relativePath) else { return nil }
        let folder = root.appendingPathComponent(relativePath)
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(
            atPath: folder.path, isDirectory: &isDir), isDir.boolValue else {
            return nil
        }
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: folder, includingPropertiesForKeys: nil) else {
            return nil
        }
        return contents.count
    }

    /// Permanently deletes the folder at `relativePath` and everything inside
    /// it (real recursive delete; there is no Trash System).
    ///
    /// Callers MUST warn the user first — a non-empty folder is wiped entirely.
    /// The folder must be a real directory under the workspace (`.minne` and
    /// `*.files` are rejected by `isValidWorkspacePath`), and `removeItem` is
    /// only called on an existing directory. On success the tree is re-scanned.
    /// Returns `false` when invalid or deletion fails.
    func deleteFolder(at relativePath: String) -> Bool {
        guard let root = workspaceURL else { return false }
        guard isValidWorkspacePath(relativePath) else { return false }
        let folder = root.appendingPathComponent(relativePath)
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(
            atPath: folder.path, isDirectory: &isDir), isDir.boolValue else {
            return false
        }
        do {
            try FileManager.default.removeItem(at: folder)
            refreshTree()
            return true
        } catch {
            return false
        }
    }

    /// Validates a single folder/note name: non-empty, no path separators,
    /// not `.`/`..`, and not an internal name (`.minne`, `*.files`).
    private func isValidFolderName(_ name: String) -> Bool {
        guard !name.isEmpty, !name.contains("/"), !name.contains(":") else { return false }
        guard name != "." && name != ".." else { return false }
        guard name != ".minne" && !name.hasSuffix(".files") else { return false }
        return true
    }

    /// Validates a Workspace-relative path: non-empty, relative, no `.`/`..`
    /// segments, and nowhere under `.minne` or a `*.files` folder.
    private func isValidWorkspacePath(_ path: String) -> Bool {
        guard !path.isEmpty, !path.hasPrefix("/") else { return false }
        let segments = (path as NSString).pathComponents
        guard !segments.contains("."), !segments.contains("..") else { return false }
        // Reject `.minne` itself or any internal segment (`.minne` / `*.files`).
        if segments.contains(".minne") || segments.contains(where: { $0.hasSuffix(".files") }) {
            return false
        }
        return true
    }

    /// Creates `<workspace>/.minne` if it does not exist yet.
    ///
    /// Idempotent: reuses an existing `.minne` directory. Returns `false` if
    /// no workspace is set or the directory cannot be created. Does not create
    /// SQLite (that is a later task).
    func ensureMinneDirectory() -> Bool {
        guard let url = minneDirectoryURL else { return false }
        guard FileManager.default.fileExists(atPath: url.path) == false else {
            // Already present — reuse it.
            return true
        }
        do {
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            return true
        } catch {
            return false
        }
    }

    /// Presents the system directory chooser and records the selection.
    ///
    /// Directory-only. On success also creates and persists a security-scoped
    /// bookmark so the workspace can be restored later (T012).
    /// Returns `true` if the user picked a directory, `false` if cancelled.
    @MainActor
    @discardableResult
    func selectWorkspace() -> Bool {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.canCreateDirectories = true
        panel.prompt = "Select Workspace"

        guard panel.runModal() == .OK, let url = panel.url else {
            return false
        }

        guard FileManager.default.fileExists(atPath: url.path) else {
            return false
        }
        guard let bookmark = createBookmark(for: url) else {
            return false
        }
        guard persist(bookmark, to: Self.bookmarkURL) else {
            return false
        }

        workspaceURL = url
        guard ensureMinneDirectory() else {
            // Cannot operate without the internal directory — do not keep the
            // selection active.
            workspaceURL = nil
            return false
        }
        refreshTree()
        openIndex()
        startWatching()
        return true
    }

    /// Restores the previously persisted workspace bookmark, if any.
    ///
    /// Called at launch. Reads `bookmarkSource` (`Self.bookmarkURL` by default),
    /// resolves it with security scope, and starts security-scoped access. On
    /// success sets `workspaceURL` and returns it. If the bookmark is missing,
    /// corrupt, or points to a now-unavailable resource, the bookmark file is
    /// removed and `nil` is returned (the UI falls back to the empty state).
    @MainActor
    @discardableResult
    func restoreWorkspace(bookmarkSource: URL? = nil) -> URL? {
        let source = bookmarkSource ?? Self.bookmarkURL
        // Nothing persisted yet.
        guard FileManager.default.fileExists(atPath: source.path) else {
            return nil
        }

        guard let bookmark = try? Data(contentsOf: source) else {
            clearStoredBookmark(at: source)
            return nil
        }

        var isStale = false
        guard let url = resolveBookmark(bookmark, isStale: &isStale) else {
            // Corrupt or unresolvable bookmark: drop it so it does not retry.
            clearStoredBookmark(at: source)
            return nil
        }

        // A stale-but-still-valid bookmark is refreshed so it stays current.
        if isStale, let fresh = createBookmark(for: url) {
            _ = persist(fresh, to: source)
        }

        // Acquire sandbox access for the workspace. In a non-sandboxed dev
        // build `startAccessing...` returns false but access is unrestricted,
        // so only treat it as a failure when sandbox is actually enabled.
        if Self.isSandboxed, !url.startAccessingSecurityScopedResource() {
            clearStoredBookmark(at: source)
            return nil
        }

        workspaceURL = url
        guard ensureMinneDirectory() else {
            clearStoredBookmark(at: source)
            return nil
        }
        refreshTree()
        openIndex()
        startWatching()
        return url
    }

    /// Resolves bookmark data to a URL, reporting whether it was stale.
    func resolveBookmark(_ bookmark: Data, isStale: inout Bool) -> URL? {
        try? URL(
            resolvingBookmarkData: bookmark,
            options: [.withSecurityScope],
            relativeTo: nil,
            bookmarkDataIsStale: &isStale
        )
    }

    /// Removes the persisted bookmark at `source` (normally `Self.bookmarkURL`),
    /// e.g. after a failed restore.
    func clearStoredBookmark(at source: URL? = nil) {
        try? FileManager.default.removeItem(at: source ?? Self.bookmarkURL)
        workspaceURL = nil
    }

    // MARK: - Bookmark

    /// Creates a security-scoped bookmark for a directory.
    ///
    /// Returns `nil` if the bookmark cannot be created.
    func createBookmark(for url: URL) -> Data? {
        try? url.bookmarkData(
            options: [.withSecurityScope, .securityScopeAllowOnlyReadAccess]
        )
    }

    /// Writes bookmark data atomically to `destination` (normally `Self.bookmarkURL`).
    ///
    /// Testable: callers pass an explicit destination. Returns `false` on failure.
    func persist(_ bookmark: Data, to destination: URL) -> Bool {
        do {
            let parent = destination.deletingLastPathComponent()
            try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
            try bookmark.write(to: destination, options: .atomic)
            return true
        } catch {
            return false
        }
    }

    /// The app's Application Support directory, created on demand.
    private static var supportDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first!
            .appendingPathComponent("Minne", isDirectory: true)
    }

    /// Whether the running process is App Sandbox–enforced.
    ///
    /// Sandboxed processes expose a sandbox container environment variable.
    /// Non-sandboxed dev builds have unrestricted file access, so
    /// `startAccessingSecurityScopedResource()` returns `false` but access
    /// still works — treat it as an error only under a real sandbox.
    static var isSandboxed: Bool {
        ProcessInfo.processInfo.environment["APP_SANDBOX_CONTAINER_ID"] != nil
    }

    /// Location of the persisted workspace bookmark.
    static var bookmarkURL: URL {
        supportDirectory.appendingPathComponent("workspace.bookmark")
    }
}