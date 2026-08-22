import XCTest
@testable import Minne
import GRDB

/// Verifies T043: `tags` and `note_tags` tables.
final class TagsSchemaTests: XCTestCase {

    private var tempDir: URL!
    private var queue: DatabaseQueue!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("minne-tags-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        queue = try? DatabaseManager.openDatabaseQueue(at: tempDir)
    }

    override func tearDown() {
        try? queue?.close()
        try? FileManager.default.removeItem(at: tempDir)
        super.tearDown()
    }

    func testTagsAndNoteTagsExist() throws {
        try queue.read { db in
            XCTAssertTrue(try db.tableExists("tags"))
            XCTAssertTrue(try db.tableExists("note_tags"))
        }
    }

    func testTagsUniqueName() throws {
        try queue.write { db in
            try db.execute(sql: "INSERT INTO tags (name) VALUES ('Swift')")
            XCTAssertThrowsError(try db.execute(sql: "INSERT INTO tags (name) VALUES ('Swift')"))
        }
    }

    func testNoteTagsCompositePrimaryKey() throws {
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO notes (id, relative_path, filename, title, folder)
                VALUES ('n1', 'a.md', 'a.md', 'A', '')
                """)
            // Insert a tag and read back its autoincrement id.
            try db.execute(sql: "INSERT INTO tags (name) VALUES ('状态机')")
            let tagID = db.lastInsertedRowID
            // Composite PK: same pair inserted twice must fail.
            try db.execute(sql: """
                INSERT INTO note_tags (note_id, tag_id) VALUES ('n1', ?)
                """, arguments: [tagID])
            XCTAssertThrowsError(try db.execute(sql: """
                INSERT INTO note_tags (note_id, tag_id) VALUES ('n1', ?)
                """, arguments: [tagID]))
        }
    }

    func testDeletingNoteCascadesTagLinks() throws {
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO notes (id, relative_path, filename, title, folder)
                VALUES ('n1', 'a.md', 'a.md', 'A', '')
                """)
            try db.execute(sql: "INSERT INTO tags (name) VALUES ('Swift')")
            let tagID = try db.lastInsertedRowID
            try db.execute(sql: """
                INSERT INTO note_tags (note_id, tag_id) VALUES ('n1', ?)
                """, arguments: [tagID])

            try db.execute(sql: "DELETE FROM notes WHERE id = 'n1'")

            let links = try Int.fetchOne(db, sql: """
                SELECT COUNT(*) FROM note_tags WHERE note_id = 'n1'
                """)
            XCTAssertEqual(links, 0, "note_tags rows must cascade away with the note")
        }
    }

    func testDeletingTagCascadesLinks() throws {
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO notes (id, relative_path, filename, title, folder)
                VALUES ('n1', 'a.md', 'a.md', 'A', '')
                """)
            try db.execute(sql: "INSERT INTO tags (name) VALUES ('旧')")
            let tagID = db.lastInsertedRowID
            try db.execute(sql: """
                INSERT INTO note_tags (note_id, tag_id) VALUES ('n1', ?)
                """, arguments: [tagID])

            try db.execute(sql: "DELETE FROM tags WHERE id = ?", arguments: [tagID])

            let links = try Int.fetchOne(db, sql: """
                SELECT COUNT(*) FROM note_tags WHERE tag_id = ?
                """, arguments: [tagID])
            XCTAssertEqual(links, 0)
        }
    }
}