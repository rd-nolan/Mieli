import Foundation
import GRDB

/// A single full-text search hit (AGENTS §28).
struct SearchResult: FetchableRecord {
    let id: String          // stable note id
    let filename: String
    let title: String
    let folder: String      // "" == workspace root
    let relativePath: String
    let snippet: String?    // matched excerpt, nil when unavailable

    init(row: Row) {
        id = row["id"]
        filename = row["filename"]
        title = row["title"]
        folder = row["folder"]
        relativePath = row["relative_path"]
        snippet = row["matched"]
    }
}

/// Full-text search over the SQLite FTS5 index (T050).
///
/// One keyword searches title, filename, path, tags and content together with
/// a single FTS5 MATCH (AGENTS §25). Ranking is deliberately basic here;
/// title/filename/tags/content priority arrives in T056. The trigram
/// tokenizer (T044) requires queries of at least 3 Unicode characters —
/// shorter queries are a no-match, not an error.
enum SearchService {

    /// Runs a full-text search, returning matching notes.
    ///
    /// `query` must be at least 3 characters long (trigram tokenizer
    /// requirement); shorter input returns an empty result set.
    static func search(_ query: String, in queue: DatabaseQueue, limit: Int = 50) throws -> [SearchResult] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        // trigram can't index tokens under 3 chars; a 1-2 char keyword matches nothing.
        guard trimmed.unicodeScalars.count >= 3 else { return [] }

        // FTS MATCH over all five columns; snippet() yields the highlighted
        // excerpt from the content column. Join notes to recover path/title.
        let sql: String = """
            SELECT n.id, n.filename, n.title, n.folder, n.relative_path,
                   snippet(note_fts, 4, '[', ']', '…', 48) AS matched
            FROM note_fts
            JOIN notes n ON n.rowid = note_fts.rowid
            WHERE note_fts MATCH ?
            -- Column-weighted bm25 (FTS5 column order: title, filename, path,
            -- tags, content). Weights follow the priority title > filename >
            -- tags > content > path (AGENTS §27) — no custom ranking engine.
            ORDER BY bm25(note_fts, 5.0, 4.0, 1.0, 3.0, 2.0)
            LIMIT ?
            """
        return try queue.read { db in
            try SearchResult.fetchAll(db, sql: sql, arguments: [trimmed, limit])
        }
    }

    /// Title-only search (T051).
    ///
    /// Matches exclusively against the FTS5 `title` column, using the
    /// column-filter syntax `title : <query>`. This lets a caller find notes
    /// by their title without conflating content/path matches. Same trigram
    /// guard as `search`: queries under 3 chars match nothing, not an error.
    static func searchTitle(_ query: String, in queue: DatabaseQueue, limit: Int = 50) throws -> [SearchResult] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.unicodeScalars.count >= 3 else { return [] }

        let sql: String = """
            SELECT n.id, n.filename, n.title, n.folder, n.relative_path,
                   snippet(note_fts, 0, '[', ']', '…', 48) AS matched
            FROM note_fts
            JOIN notes n ON n.rowid = note_fts.rowid
            WHERE note_fts MATCH 'title : ' || ?
            ORDER BY bm25(note_fts)
            LIMIT ?
            """
        return try queue.read { db in
            try SearchResult.fetchAll(db, sql: sql, arguments: [trimmed, limit])
        }
    }

    /// Filename-only search (T052).
    ///
    /// Matches exclusively against the FTS5 `filename` column (e.g. `hello.md`),
    /// reusing the same column-filter + trigram guard as `searchTitle`.
    static func searchFilename(_ query: String, in queue: DatabaseQueue, limit: Int = 50) throws -> [SearchResult] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.unicodeScalars.count >= 3 else { return [] }

        let sql: String = """
            SELECT n.id, n.filename, n.title, n.folder, n.relative_path,
                   snippet(note_fts, 1, '[', ']', '…', 48) AS matched
            FROM note_fts
            JOIN notes n ON n.rowid = note_fts.rowid
            WHERE note_fts MATCH 'filename : ' || ?
            ORDER BY bm25(note_fts)
            LIMIT ?
            """
        return try queue.read { db in
            try SearchResult.fetchAll(db, sql: sql, arguments: [trimmed, limit])
        }
    }

    /// Tag-only search (T053).
    ///
    /// Matches exclusively against the FTS5 `tags` column, reusing the same
    /// column-filter + trigram guard as the other field-scoped searches.
    static func searchTags(_ query: String, in queue: DatabaseQueue, limit: Int = 50) throws -> [SearchResult] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.unicodeScalars.count >= 3 else { return [] }

        let sql: String = """
            SELECT n.id, n.filename, n.title, n.folder, n.relative_path,
                   snippet(note_fts, 3, '[', ']', '…', 48) AS matched
            FROM note_fts
            JOIN notes n ON n.rowid = note_fts.rowid
            WHERE note_fts MATCH 'tags : ' || ?
            ORDER BY bm25(note_fts)
            LIMIT ?
            """
        return try queue.read { db in
            try SearchResult.fetchAll(db, sql: sql, arguments: [trimmed, limit])
        }
    }

    /// Content-only search (T054).
    ///
    /// Matches exclusively against the FTS5 `content` column, reusing the same
    /// column-filter + trigram guard as the other field-scoped searches.
    static func searchContent(_ query: String, in queue: DatabaseQueue, limit: Int = 50) throws -> [SearchResult] {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.unicodeScalars.count >= 3 else { return [] }

        let sql: String = """
            SELECT n.id, n.filename, n.title, n.folder, n.relative_path,
                   snippet(note_fts, 4, '[', ']', '…', 48) AS matched
            FROM note_fts
            JOIN notes n ON n.rowid = note_fts.rowid
            WHERE note_fts MATCH 'content : ' || ?
            ORDER BY bm25(note_fts)
            LIMIT ?
            """
        return try queue.read { db in
            try SearchResult.fetchAll(db, sql: sql, arguments: [trimmed, limit])
        }
    }
}