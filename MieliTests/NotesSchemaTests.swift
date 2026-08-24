import XCTest
@testable import Mieli
import GRDB

/// Verifies T042: `notes` table schema creation.
final class NotesSchemaTests: XCTestCase {

    private var tempDir: URL!
    private var queue: DatabaseQueue!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mieli-schema-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        queue = try? DatabaseManager.openDatabaseQueue(at: tempDir)
    }

    override func tearDown() {
        try? queue?.close()
        try? FileManager.default.removeItem(at: tempDir)
        super.tearDown()
    }

    func testNotesTableExists() throws {
        try queue.read { db in
            let exists = try db.tableExists("notes")
            XCTAssertTrue(exists)
        }
    }

    func testNotesHasAllColumns() throws {
        try queue.read { db in
            let cols = try db.columns(in: "notes").map(\.name)
            XCTAssertEqual(Set(cols), Set([
                "id", "relative_path", "filename", "title", "folder",
                "created_at", "updated_at", "file_mtime", "file_size", "content_hash",
            ]))
        }
    }

    func testIdIsPrimaryKey() throws {
        try queue.read { db in
            let pk = try db.primaryKey("notes")
            XCTAssertEqual(pk.columns, ["id"])
        }
    }

    func testRelativePathIsUnique() throws {
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO notes (id, relative_path, filename, title, folder)
                VALUES ('a', 'x.md', 'x.md', 'X', '')
                """)
            // Same relative_path → unique constraint must reject.
            XCTAssertThrowsError(try db.execute(sql: """
                INSERT INTO notes (id, relative_path, filename, title, folder)
                VALUES ('b', 'x.md', 'x.md', 'X', '')
                """))
        }
    }

    func testMigrationsAreIdempotentOnReopen() throws {
        try queue.close()
        // Re-open: migrations run again but are no-ops (already applied).
        let q2 = try DatabaseManager.openDatabaseQueue(at: tempDir)
        try q2.read { db in
            XCTAssertTrue(try db.tableExists("notes"))
        }
        try q2.close()
    }
}