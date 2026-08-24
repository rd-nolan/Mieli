import Foundation
import GRDB

/// Rebuilds the SQLite search index from the Markdown files in a Workspace
/// (T048).
///
/// This is an *index rebuild*, never a "sync". It clears the index and
/// re-indexes every `*.md` file it finds. It never modifies Markdown — the
/// rebuild is strictly a read of the workspace into the database.
enum IndexRebuilder {

    /// Rebuilds the index for `workspaceURL`.
    ///
    /// Scans for Markdown, reads each file, then replaces the whole index in a
    /// single transaction. File reads happen before the write transaction so
    /// the database lock is held only for the (fast) pure-DB work.
    static func rebuild(workspaceURL: URL, in queue: DatabaseQueue) throws {
        let notes = try collectNotes(at: workspaceURL)
        try queue.write { db in
            try clear(db)
            for note in notes {
                try IndexService.record(note, into: db)
            }
        }
    }

    /// Reads every Markdown file under `root` into a parsed note. Files that
    /// vanish mid-scan or fail to decode are skipped (they will be re-caught
    /// on the next rebuild); the rebuild never fails over one bad file.
    private static func collectNotes(at root: URL) throws -> [ParsedNote] {
        let items = try WorkspaceScanner.scan(root)
        var notes: [ParsedNote] = []
        collectNotes(items, root: root, into: &notes)
        return notes
    }

    private static func collectNotes(_ items: [WorkspaceItem], root: URL, into notes: inout [ParsedNote]) {
        for item in items {
            switch item.kind {
            case .folder:
                collectNotes(item.children ?? [], root: root, into: &notes)
            case .note:
                let url = root.appendingPathComponent(item.relativePath)
                guard let data = try? Data(contentsOf: url),
                      let markdown = String(data: data, encoding: .utf8) else { continue }
                notes.append(ParsedNote(relativePath: item.relativePath, markdown: markdown))
            }
        }
    }

    /// Empties all index tables. `DELETE FROM notes` cascades away `note_tags`;
    /// deleting `tags` next also clears now-orphaned tag names.
    private static func clear(_ db: Database) throws {
        try db.execute(sql: "DELETE FROM note_fts")
        try db.execute(sql: "DELETE FROM tags")
        try db.execute(sql: "DELETE FROM notes")
    }
}