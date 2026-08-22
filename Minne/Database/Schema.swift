import Foundation
import GRDB

/// Database migrations for the local, rebuildable index (AGENTS §23).
enum Schema {

    /// Returns the migrator that builds all current tables.
    static func makeMigrator() -> DatabaseMigrator {
        var migrator = DatabaseMigrator()

migrator.registerMigration("createNotes") { db in
            try db.create(table: "notes") { t in
                t.column("id", .text).primaryKey()          // stable ULID note id
                t.column("relative_path", .text).notNull().unique() // workspace-relative
                t.column("filename", .text).notNull()       // e.g. hello.md
                t.column("title", .text).notNull()
                t.column("folder", .text).notNull()         // "" == workspace root
                t.column("created_at", .text)               // ISO-8601
                t.column("updated_at", .text)
                t.column("file_mtime", .double)
                t.column("file_size", .integer)
                t.column("content_hash", .text)
            }
        }

        migrator.registerMigration("createTags") { db in
            try db.create(table: "tags") { t in
                t.autoIncrementedPrimaryKey("id")
                t.column("name", .text).notNull().unique()
            }
            // Many-to-many link: a note may carry several tags, a tag many notes.
            // Deleting a note (or a tag) removes its tag links automatically.
            try db.create(table: "note_tags") { t in
                t.column("note_id", .text)
                t.column("tag_id", .integer)
                t.primaryKey(["note_id", "tag_id"])
                t.foreignKey(["note_id"], references: "notes",
                             columns: ["id"], onDelete: .cascade)
                t.foreignKey(["tag_id"], references: "tags",
                             columns: ["id"], onDelete: .cascade)
            }
        }

        migrator.registerMigration("createNoteFTS") { db in
            // Full-text search index (AGENTS §25). trigram tokenizer gives
            // substring + Chinese support without external components (AGENTS §26).
            try db.execute(sql: """
                CREATE VIRTUAL TABLE note_fts USING fts5(
                    title,
                    filename,
                    path,
                    tags,
                    content,
                    tokenize = 'trigram'
                )
                """)
        }

        return migrator
    }
}