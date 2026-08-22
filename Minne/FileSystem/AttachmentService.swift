import Foundation

/// Manages the per-note attachment folder `<note>.files/` (AGENTS §18, T080).
///
/// A note `技术方案.md` keeps its attachments in the sibling directory
/// `技术方案.files/` on disk. Only directory *resolution + creation* lives
/// here for now; copying dropped files and writing Markdown links are later
/// tasks (T081–T086).
enum AttachmentService {

    /// Reasons a dropped file cannot be copied into a note's attachment folder.
    enum CopyError: Error, Equatable {
        /// An item with the same name already exists in the folder. Never
        /// silently overwrite user data (AGENTS §5); unique-name handling is
        /// a later task (T082).
        case fileExists
        /// The copy failed (I/O, permission, …).
        case copyFailed
    }

    /// Copies `sourceURL` into the note's attachment folder, returning the
    /// destination URL (T081). Refuses to overwrite an existing item.
    ///
    /// - Note: the returned URL is absolute; callers that write a Markdown
    ///   relative link must relativize it against the note (T083+).
    static func copyAttachment(from sourceURL: URL,
                               forNoteRelativePath noteRelativePath: String,
                               in root: URL) throws -> URL {
        let folder = try attachmentDirectory(forNoteRelativePath: noteRelativePath, in: root)
        let destination = folder.appendingPathComponent(sourceURL.lastPathComponent)

        var isDir: ObjCBool = false
        let exists = FileManager.default.fileExists(atPath: destination.path, isDirectory: &isDir)
        if exists {
            throw CopyError.fileExists
        }
        do {
            try FileManager.default.copyItem(at: sourceURL, to: destination)
        } catch {
            throw CopyError.copyFailed
        }
        return destination
    }

    /// Returns a filename inside `directory` that does not yet exist, by
    /// appending `-1`, `-2`, … to the base name on collision (T082).
    ///
    /// `image.png` → `image.png` (free) / `image-1.png` / `image-2.png` when
    /// take. A name with no extension gets `name-1`. Never overwrites.
    static func availableName(preferredName: String, in directory: URL) -> String {
        let ns = preferredName as NSString
        let base = ns.deletingPathExtension
        let ext = ns.pathExtension
        var candidate = preferredName
        var i = 1
        while FileManager.default.fileExists(
            atPath: directory.appendingPathComponent(candidate).path) {
            candidate = ext.isEmpty ? "\(base)-\(i)" : "\(base)-\(i).\(ext)"
            i += 1
        }
        return candidate
    }

    /// Copies `sourceURL` into the note's attachment folder, auto-uniquing the
    /// destination filename so an existing item is never overwritten (T82).
    /// Returns the actual destination URL.
    static func copyAttachmentUnique(from sourceURL: URL,
                                     forNoteRelativePath noteRelativePath: String,
                                     in root: URL) throws -> URL {
        let folder = try attachmentDirectory(forNoteRelativePath: noteRelativePath, in: root)
        let name = availableName(preferredName: sourceURL.lastPathComponent, in: folder)
        let destination = folder.appendingPathComponent(name)
        do {
            try FileManager.default.copyItem(at: sourceURL, to: destination)
        } catch {
            throw CopyError.copyFailed
        }
        return destination
    }

    /// Returns a note's attachment folder name for a base stem: `"洗碗方案"`
    /// → `"洗碗方案.files"` (AGENTS §18).
    static func attachmentFolderName(forNoteStem stem: String) -> String {
        stem + ".files"
    }

    /// Renames a note's attachment folder when the note is renamed (T086):
    /// `<stem>.files` moves to `<newStem>.files`, in place, under `root`.
    ///
    /// - Returns:
    ///   - `true` when the folder was moved (or already had the new name).
    ///   - `false` when the old folder exists but the rename must not happen
    ///     (destination already exists — never merge/overwrite) — the caller
    ///     keeps the old folder and links intact (no data loss).
    static func renameAttachmentFolder(fromNoteStem oldStem: String,
                                       toNoteStem newStem: String,
                                       in root: URL) -> Bool {
        let oldFolder = root.appendingPathComponent(attachmentFolderName(forNoteStem: oldStem))
        let newFolder = root.appendingPathComponent(attachmentFolderName(forNoteStem: newStem))
        guard oldFolder.path != newFolder.path else { return true }
        let hasOld = FileManager.default.fileExists(atPath: oldFolder.path)
        guard hasOld else { return true } // nothing to rename
        // Never overwrite an existing attachment folder (data safety).
        if FileManager.default.fileExists(atPath: newFolder.path) { return false }
        do {
            try FileManager.default.moveItem(at: oldFolder, to: newFolder)
            return true
        } catch {
            return false
        }
    }

    /// Rewrites relative attachment paths in a note's markdown after the note
    /// is renamed (T086): every `./<oldStem>.files/…` (and `<oldStem>.files/…`)
    /// becomes `./<newStem>.files/…`. Pure and safe on unrelated content.
    static func rewritingAttachmentLinks(_ markdown: String,
                                         oldStem: String,
                                         newStem: String) -> String {
        guard oldStem != newStem else { return markdown }
        let old = oldStem + ".files"
        let new = newStem + ".files"
        // Replace both `./old.files/` and bare `old.files/` path prefixes.
        let withDot = markdown.replacingOccurrences(
            of: "./\(old)/", with: "./\(new)/")
        return withDot.replacingOccurrences(of: "\(old)/", with: "\(new)/")
    }

    /// Returns the on-disk attachment directory for a note, **creating it if
    /// it does not yet exist**.
    ///
    /// - Parameters:
    ///   - noteRelativePath: workspace-relative path to the note, e.g.
    ///     `工作/技术方案.md`.
    ///   - root: the workspace root URL.
    /// - Throws: `FileService.SaveFailure.missingTarget` when the note's parent
    ///   directory is missing or not a directory (the note cannot exist there).
    static func attachmentDirectory(forNoteRelativePath noteRelativePath: String,
                                    in root: URL) throws -> URL {
        let noteURL = root.appendingPathComponent(noteRelativePath)

        var isDir: ObjCBool = false
        let noteDir = noteURL.deletingLastPathComponent()
        guard FileManager.default.fileExists(atPath: noteDir.path, isDirectory: &isDir),
              isDir.boolValue else {
            throw FileService.SaveFailure.missingTarget
        }

        let folderName = noteURL.deletingPathExtension().lastPathComponent + ".files"
        let dir = noteDir.appendingPathComponent(folderName, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }
}