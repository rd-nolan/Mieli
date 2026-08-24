import XCTest
@testable import Mieli
import GRDB

/// Verifies T041: SQLite index database creation at `<workspace>/.mieli/index.sqlite`.
final class DatabaseManagerTests: XCTestCase {

    private var tempDir: URL!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mieli-db-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempDir)
        super.tearDown()
    }

    func testOpensIndexFileUnderDotMieli() throws {
        let queue = try DatabaseManager.openDatabaseQueue(at: tempDir)
        let file = tempDir.appendingPathComponent(".mieli/index.sqlite")
        XCTAssertTrue(FileManager.default.fileExists(atPath: file.path))
        try queue.close()
    }

    func testCreatesDotMieliDirectoryIdempotently() throws {
        // Workspace exists, but no `.mieli` yet.
        let dir = tempDir.appendingPathComponent(".mieli")
        XCTAssertFalse(FileManager.default.fileExists(atPath: dir.path))
        let queue = try DatabaseManager.openDatabaseQueue(at: tempDir)
        XCTAssertTrue(FileManager.default.fileExists(atPath: dir.path))
        try queue.close()
    }

    func testMigratesLegacyDotMuistiDirectoryWithoutDiscardingContents() throws {
        let legacy = tempDir.appendingPathComponent(".muisti", isDirectory: true)
        try FileManager.default.createDirectory(at: legacy, withIntermediateDirectories: true)
        let marker = legacy.appendingPathComponent("marker.txt")
        try Data("keep".utf8).write(to: marker)

        let queue = try DatabaseManager.openDatabaseQueue(at: tempDir)
        defer { try? queue.close() }

        let current = tempDir.appendingPathComponent(".mieli", isDirectory: true)
        XCTAssertTrue(FileManager.default.fileExists(atPath: current.path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: legacy.path))
        XCTAssertEqual(try String(contentsOf: current.appendingPathComponent("marker.txt")), "keep")
    }

    func testMigratesHistoricalDotMinneDirectoryWithoutDiscardingContents() throws {
        let legacy = tempDir.appendingPathComponent(".minne", isDirectory: true)
        try FileManager.default.createDirectory(at: legacy, withIntermediateDirectories: true)
        let marker = legacy.appendingPathComponent("marker.txt")
        try Data("keep".utf8).write(to: marker)

        let queue = try DatabaseManager.openDatabaseQueue(at: tempDir)
        defer { try? queue.close() }

        let current = tempDir.appendingPathComponent(".mieli", isDirectory: true)
        XCTAssertTrue(FileManager.default.fileExists(atPath: current.path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: legacy.path))
        XCTAssertEqual(try String(contentsOf: current.appendingPathComponent("marker.txt")), "keep")
    }

    func testOpenIsIdempotent() throws {
        let q1 = try DatabaseManager.openDatabaseQueue(at: tempDir)
        try q1.close()
        // Re-opening the same workspace must succeed and reuse the file.
        let q2 = try DatabaseManager.openDatabaseQueue(at: tempDir)
        try q2.close()
        let file = tempDir.appendingPathComponent(".mieli/index.sqlite")
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
