import XCTest
@testable import Mieli
import GRDB

/// Verifies T044: FTS5 `note_fts` table + trigram tokenizer support on the
/// embedded SQLite (including Chinese substring search, AGENTS §25/§26).
final class FTSSchemaTests: XCTestCase {

    private var tempDir: URL!
    private var queue: DatabaseQueue!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mieli-fts-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        queue = try? DatabaseManager.openDatabaseQueue(at: tempDir)
    }

    override func tearDown() {
        try? queue?.close()
        try? FileManager.default.removeItem(at: tempDir)
        super.tearDown()
    }

    func testNoteFTSExists() throws {
        try queue.read { db in
            XCTAssertTrue(try db.tableExists("note_fts"))
        }
    }

    /// Inserts a fake note row + a matching FTS row is not needed here —
    /// rows are inserted per test; this just runs a MATCH against a given rowid.
    private func match(_ query: String) throws -> Bool {
        try queue.read { db in
            let rowID = try Int.fetchOne(db, sql: "SELECT rowid FROM note_fts LIMIT 1") ?? 0
            let n = try Int.fetchOne(db, sql: """
                SELECT count(*) FROM note_fts WHERE note_fts MATCH ? AND rowid = ?
                """, arguments: [query, rowID])
            return n == 1
        }
    }

    func testTrigramTokenizerCreatedAndChineseContentMatches() throws {
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO note_fts (title, filename, path, tags, content)
                VALUES ('状态机', 'spring.md', '工作/spring.md', 'Swift', '今天研究了 Spring 状态机实现方案')
                """)
        }
        XCTAssertTrue(try match("状态机"))
    }

    func testChinesePhraseSubstringMatches() throws {
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO note_fts (title, filename, path, tags, content)
                VALUES ('t', 'a.md', 'a.md', '', '实现方案要点')
                """)
        }
        XCTAssertTrue(try match("实现方案"))
    }

    func testEnglishWordMatches() throws {
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO note_fts (title, filename, path, tags, content)
                VALUES ('t', 'a.md', 'a.md', '', 'Today studied the Spring state machine')
                """)
        }
        XCTAssertTrue(try match("spring"))
    }

    func testSearchAcrossTagsColumn() throws {
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO note_fts (title, filename, path, tags, content)
                VALUES ('t', 'a.md', 'a.md', 'macOS Swift', 'body')
                """)
        }
        XCTAssertTrue(try match("macOS"))
    }

    func testSearchAcrossTitleColumn() throws {
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO note_fts (title, filename, path, tags, content)
                VALUES ('状态机设计', 'a.md', 'a.md', '', 'body')
                """)
        }
        XCTAssertTrue(try match("状态机"))
    }
}