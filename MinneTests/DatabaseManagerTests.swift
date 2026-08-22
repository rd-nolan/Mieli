import XCTest
@testable import Minne
import GRDB

/// Verifies T041: SQLite index database creation at `<workspace>/.minne/index.sqlite`.
final class DatabaseManagerTests: XCTestCase {

    private var tempDir: URL!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("minne-db-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempDir)
        super.tearDown()
    }

    func testOpensIndexFileUnderDotMinne() throws {
        let queue = try DatabaseManager.openDatabaseQueue(at: tempDir)
        let file = tempDir.appendingPathComponent(".minne/index.sqlite")
        XCTAssertTrue(FileManager.default.fileExists(atPath: file.path))
        try queue.close()
    }

    func testCreatesDotMinneDirectoryIdempotently() throws {
        // Workspace exists, but no `.minne` yet.
        let dir = tempDir.appendingPathComponent(".minne")
        XCTAssertFalse(FileManager.default.fileExists(atPath: dir.path))
        let queue = try DatabaseManager.openDatabaseQueue(at: tempDir)
        XCTAssertTrue(FileManager.default.fileExists(atPath: dir.path))
        try queue.close()
    }

    func testOpenIsIdempotent() throws {
        let q1 = try DatabaseManager.openDatabaseQueue(at: tempDir)
        try q1.close()
        // Re-opening the same workspace must succeed and reuse the file.
        let q2 = try DatabaseManager.openDatabaseQueue(at: tempDir)
        try q2.close()
        let file = tempDir.appendingPathComponent(".minne/index.sqlite")
        XCTAssertTrue(FileManager.default.fileExists(atPath: file.path))
    }

    func testJournalModeIsWAL() throws {
        let queue = try DatabaseManager.openDatabaseQueue(at: tempDir)
        try queue.read { db in
            let mode = try String.fetchOne(db, sql: "PRAGMA journal_mode")
            XCTAssertEqual(mode?.lowercased(), "wal")
        }
        try queue.close()
    }
}