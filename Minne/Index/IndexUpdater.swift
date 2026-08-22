import Foundation
import GRDB
import CryptoKit

/// Reconciles the SQLite index against the current Workspace on startup (T049).
///
/// Compares the stored `relative_path` + `file_mtime` + `file_size` for each
/// Markdown file to avoid re-indexing everything on every launch:
///
/// - new file            → index
/// - modified (mtime/size differ) → reindex
/// - deleted file        → remove index
/// - unchanged           → skip
///
/// Only modified/new files are re-read; unchanged files are never opened, which
/// is what keeps startup fast. Stored `content_hash` lets a full verify happen
/// later without touching mtime equality.
enum IndexUpdater {

    private struct IndexedRow {
        let id: String
        let mtime: Double?   // TimeInterval since reference date; nil if never set
        let size: Int64?
    }

    /// Refreshes the index from the workspace with minimal work. Never modifies
    /// Markdown. Throws on a scanning or database failure; individual unreadable
    /// files are skipped, not fatal.
    static func reconcile(workspace: URL, in queue: DatabaseQueue) throws {
        let items = try WorkspaceScanner.scan(workspace)
        var paths: [String] = []
        flattenNotePaths(items, into: &paths)

        var indexed: [String: IndexedRow] = [:]
        try queue.read { db in
            let rows = try Row.fetchAll(db, sql: """
                SELECT id, relative_path, file_mtime, file_size FROM notes
                """)
            for row in rows {
                let path = row["relative_path"] as String
                indexed[path] = IndexedRow(
                    id: row["id"] as String,
                    mtime: row["file_mtime"] as Double?,
                    size: row["file_size"] as Int64?
                )
            }
        }

        var seen = Set<String>()
        var toTouch: [(id: String, mtime: Double, size: Int64, hash: String)] = []
        var toDelete: [String] = []

        for path in paths {
            let url = workspace.appendingPathComponent(path)
            let values: URLResourceValues
            do {
                values = try url.resourceValues(forKeys: [.contentModificationDateKey, .fileSizeKey])
            } catch {
                continue // vanished between scan and stat
            }
            let mtime = values.contentModificationDate?.timeIntervalSince1970 ?? 0
            let size = Int64(values.fileSize ?? 0)

            seen.insert(path)

            if let row = indexed[path], row.mtime == mtime, row.size == size {
                continue                          // unchanged → skip
            }

            guard let content = try? String(contentsOf: url, encoding: .utf8) else { continue }
            let note = ParsedNote(relativePath: path, markdown: content)

            if indexed[path] != nil {
                try IndexService.update(note, in: queue)   // modified
            } else {
                try IndexService.index(note, in: queue)     // new
            }
            toTouch.append((note.id, mtime, size, Self.hash(content)))
        }

        // Deleted files: indexed rows whose path is no longer on disk.
        for path in indexed.keys where !seen.contains(path) {
            toDelete.append(path)
        }

        // Apply file-system metadata + deletions in one transaction.
        try queue.write { db in
            for (id, mtime, size, hash) in toTouch {
                try db.execute(sql: """
                    UPDATE notes SET file_mtime = ?, file_size = ?, content_hash = ?
                    WHERE id = ?
                    """, arguments: [mtime, size, hash, id])
            }
            for path in toDelete {
                try db.execute(sql: "DELETE FROM notes WHERE relative_path = ?", arguments: [path])
            }
        }
    }

    /// Re-indexes a single note after it has been saved (T066) without
    /// rescanning the whole workspace. Reads the file, refreshes metadata +
    /// full-text via `IndexService.update`, then rewrites `file_mtime` /
    /// `file_size` / `content_hash` so the next startup reconcile sees the file
    /// as unchanged and skips it. No-op when the file is missing. Never
    /// modifies Markdown.
    static func updateFile(at relativePath: String, workspace: URL, in queue: DatabaseQueue) throws {
        let url = workspace.appendingPathComponent(relativePath)
        guard let content = try? String(contentsOf: url, encoding: .utf8) else { return }

        let note = ParsedNote(relativePath: relativePath, markdown: content)
        // The note may be newly discovered externally (no indexed row yet):
        // `update` locates by stable id, so fall back to a fresh insert when
        // the id isn't indexed yet (T094). Existing ids keep their row across
        // renames/moves (AGENTS §10).
        do {
            try IndexService.update(note, in: queue)
        } catch IndexService.IndexServiceError.noteNotIndexed {
            try IndexService.index(note, in: queue)
        } catch {
            throw error
        }

        let values = try? url.resourceValues(forKeys: [.contentModificationDateKey, .fileSizeKey])
        let mtime = values?.contentModificationDate?.timeIntervalSince1970 ?? 0
        let size = Int64(values?.fileSize ?? 0)
        let hash = Self.hash(content)
        try queue.write { db in
            try db.execute(sql: """
                UPDATE notes SET file_mtime = ?, file_size = ?, content_hash = ?
                WHERE id = ?
                """, arguments: [mtime, size, hash, note.id])
        }
    }

    /// SHA-256 hex of the Markdown content, for change detection.
    private static func hash(_ content: String) -> String {
        let digest = SHA256.hash(data: Data(content.utf8))
        return digest.map { String(format: "%02x", $0) }.joined()
    }

    private static func flattenNotePaths(_ items: [WorkspaceItem], into into: inout [String]) {
        for item in items {
            switch item.kind {
            case .folder:
                flattenNotePaths(item.children ?? [], into: &into)
            case .note:
                into.append(item.relativePath)
            }
        }
    }
}