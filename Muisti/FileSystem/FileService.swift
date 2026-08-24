import Foundation

/// Safe, atomic Markdown persistence (AGENTS §14).
///
/// Writes the full content to a sibling `.<name>.tmp` file in the **same**
/// directory (same volume), then atomically replaces the target with it. If
/// anything fails before replacement, the original file is left untouched and
/// the temporary file is cleaned up — a crash or disk error can never leave a
/// partially written or emptied note.
enum FileService {
    /// Outcome of a save attempt.
    enum SaveFailure: Error, Equatable {
        /// The target directory is missing or not a directory.
        case missingTarget
        /// Writing the temporary file failed (permission, disk full, …).
        case tempWriteFailed
        /// Replacing the target with the temporary file failed.
        case replaceFailed
    }

    /// Atomically writes `content` to `url`.
    ///
    /// - Returns: `true` on success. On failure the original file (if any) is
    ///   unchanged and a detailed `SaveFailure` is thrown.
    @discardableResult
    static func saveMarkdown(_ content: String, to url: URL) throws -> Bool {
        let directory = url.deletingLastPathComponent()
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(
            atPath: directory.path, isDirectory: &isDir), isDir.boolValue else {
            throw SaveFailure.missingTarget
        }

        // Temporary file lives beside the target so the eventual replace stays
        // on the same volume (a cross-volume rename is not atomic).
        let tempURL = directory.appendingPathComponent(".\(url.lastPathComponent).tmp")

        defer { try? FileManager.default.removeItem(at: tempURL) }

        do {
            try content.write(to: tempURL, atomically: true, encoding: .utf8)
        } catch {
            throw SaveFailure.tempWriteFailed
        }

        // Atomic replace: the target is swapped in one operation; a crash in
        // between leaves either the old or the new file, never a truncated one.
        do {
            _ = try FileManager.default.replaceItemAt(url, withItemAt: tempURL)
        } catch {
            throw SaveFailure.replaceFailed
        }
        return true
    }
}