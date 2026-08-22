import Foundation
import GRDB

/// Indexes parsed notes into the local SQLite search index (AGENTS §23, T045).
///
/// This is an *index update* (never a "sync"). It writes three cooperating
/// stores in one transaction: the `notes` metadata row, the `tags`/`note_tags`
/// links, and the FTS5 `note_fts` row.
enum IndexService {

    enum IndexServiceError: Error, Equatable {
        /// `update` was called for a note that has not been indexed yet.
        case noteNotIndexed
    }

/// Removes a note from the index after its file was deleted (T047).
    ///
    /// Locates the row by its workspace-relative path and deletes the FTS row
    /// plus the notes row (whose tag links cascade away via FK). Idempotent —
    /// removing an already-missing note is a no-op.
    static func remove(relativePath: String, in queue: DatabaseQueue) throws {
        try queue.write { db in
            // 1) FTS5 row (no FK — delete explicitly by notes rowid).
            try db.execute(sql: """
                DELETE FROM note_fts
                WHERE rowid IN (SELECT rowid FROM notes WHERE relative_path = ?)
                """, arguments: [relativePath])
            // 2) notes row; note_tags links cascade via ON DELETE CASCADE.
            try db.execute(sql: "DELETE FROM notes WHERE relative_path = ?", arguments: [relativePath])
        }
    }

    /// Refreshes an already-indexed note after its Markdown changed (T046).
    ///
    /// Updates the notes metadata row, reconciles tag links to the current
    /// `note.tags`, and rewrites the FTS5 row — all in one transaction. The
    /// note's stable `id` locates the row (rename/move keep the id, AGENTS §10).
    static func update(_ note: ParsedNote, in queue: DatabaseQueue) throws {
        try queue.write { db in
            // 1) notes metadata row. Missing row → not indexed yet.
            try db.execute(sql: """
                UPDATE notes
                SET filename = ?, title = ?, folder = ?, created_at = ?, updated_at = ?
                WHERE id = ?
                """, arguments: [note.filename, note.title, note.folder,
                                 note.createdAtISO, note.updatedAtISO, note.id])
            if db.changesCount == 0 {
                throw IndexServiceError.noteNotIndexed
            }

            // 2) reconcile tag links against the current tag set.
            try db.execute(sql: "DELETE FROM note_tags WHERE note_id = ?", arguments: [note.id])
            for tag in note.tags {
                try db.execute(sql: "INSERT OR IGNORE INTO tags (name) VALUES (?)", arguments: [tag])
                if let tagID = try Int.fetchOne(db, sql: "SELECT id FROM tags WHERE name = ?", arguments: [tag]) {
                    try db.execute(sql: """
                        INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?, ?)
                        """, arguments: [note.id, tagID])
                }
            }

            // 3) full-text row, keyed by the notes rowid.
            try db.execute(sql: """
                UPDATE note_fts
                SET title = ?, filename = ?, path = ?, tags = ?, content = ?
                WHERE rowid = (SELECT rowid FROM notes WHERE id = ?)
                """, arguments: [note.title, note.filename, note.relativePath,
                                 note.tags.joined(separator: " "), note.plainText, note.id])
        }
    }

    /// Inserts a note into the index (metadata + tags + full-text row).
    ///
    /// - Parameters:
    ///   - note: the parsed note to index.
    ///   - queue: the database to write into.
    /// - Throws: on database failure. The operation is transactional: either
    ///   all three stores are written or none.
static func index(_ note: ParsedNote, in queue: DatabaseQueue) throws {
        // `queue.write` already runs in a single transaction: a throw rolls
        // back every insert. The three stores stay consistent as one unit.
        try queue.write { db in
            try record(note, into: db)
        }
    }

    /// Writes one note into all three stores, operating on an already-open
    /// database inside a caller-owned transaction. Shared by `index` (single
    /// note) and `IndexRebuilder` (bulk rebuild).
    static func record(_ note: ParsedNote, into db: Database) throws {
            // 1) notes metadata row
            try db.execute(sql: """
                INSERT INTO notes (id, relative_path, filename, title, folder, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                """, arguments: [note.id, note.relativePath, note.filename,
                                 note.title, note.folder, note.createdAtISO, note.updatedAtISO])
            // Capture the notes.rowid immediately: later inserts (tags below)
            // would overwrite `lastInsertedRowID` and break the FTS row link.
            let noteRowID = Int(db.lastInsertedRowID)

            // 2) tags + tag links (create-if-missing, link-if-missing)
            for tag in note.tags {
                try db.execute(sql: "INSERT OR IGNORE INTO tags (name) VALUES (?)", arguments: [tag])
                if let tagID = try Int.fetchOne(db, sql: "SELECT id FROM tags WHERE name = ?", arguments: [tag]) {
                    try db.execute(sql: """
                        INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?, ?)
                        """, arguments: [note.id, tagID])
                }
            }

            // 3) FTS5 row keyed by the notes.rowid.
            try db.execute(sql: """
                INSERT INTO note_fts (rowid, title, filename, path, tags, content)
                VALUES (?, ?, ?, ?, ?, ?)
                """, arguments: [noteRowID, note.title, note.filename, note.relativePath,
                                 note.tags.joined(separator: " "), note.plainText])
    }

    /// Returns every note carrying `tag`, sorted by filename (T074).
    static func taggedNotes(tag: String, in queue: DatabaseQueue) throws -> [TaggedNote] {
        let sql = """
            SELECT n.id, n.filename, n.title, n.folder, n.relative_path
            FROM notes n
            JOIN note_tags nt ON nt.note_id = n.id
            JOIN tags t ON t.id = nt.tag_id
            WHERE t.name = ?
            ORDER BY n.filename COLLATE NOCASE
            """
        return try queue.read { db in
            try Row.fetchAll(db, sql: sql, arguments: [tag]).map { row in
                TaggedNote(
                    id: row["id"],
                    filename: row["filename"],
                    title: row["title"],
                    folder: row["folder"],
                    relativePath: row["relative_path"]
                )
            }
        }
    }
}