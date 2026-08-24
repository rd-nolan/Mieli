import XCTest
@testable import Muisti

/// Verifies T011: `WorkspaceManager` creates a security-scoped bookmark for a
/// workspace URL and persists it so the data round-trips through disk.
///
/// Resolving a security-scoped bookmark fully only works inside a sandboxed
/// app, so these tests validate the create + persist contract that T012's
/// restore path depends on (non-empty data, atomic disk round-trip).
final class WorkspaceBookmarkTests: XCTestCase {

    private var tempDir: URL!
    private var bookmarkFile: URL!

    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        bookmarkFile = tempDir.appendingPathComponent("workspace.bookmark")
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: tempDir)
        tempDir = nil
        bookmarkFile = nil
    }

    func testManagerCreatesBookmarkForDirectory() {
        let manager = WorkspaceManager()
        let data = manager.createBookmark(for: tempDir)
        XCTAssertNotNil(data)
        XCTAssertFalse(data!.isEmpty, "bookmark data should be non-empty")
    }

    func testManagerPersistsBookmarkRoundTrip() throws {
        let manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))

        XCTAssertTrue(manager.persist(data, to: bookmarkFile))

        let loaded = try Data(contentsOf: bookmarkFile)
        XCTAssertEqual(loaded, data, "persisted bookmark should match in memory")
    }

    func testPersistCreatesParentDirectory() throws {
        let manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))

        // Target a path whose parent directory does not exist yet.
        let nested = tempDir.appendingPathComponent("nested/path/workspace.bookmark")
        XCTAssertTrue(manager.persist(data, to: nested))
        XCTAssertTrue(FileManager.default.fileExists(atPath: nested.path))
    }
}

// MARK: - T012 restore

extension WorkspaceBookmarkTests {

    /// No persisted bookmark → restore returns nil and leaves state empty.
    @MainActor
    func testRestoreWithNoBookmarkReturnsNil() {
        let manager = WorkspaceManager()
        let source = tempDir.appendingPathComponent("does-not-exist.bookmark")

        XCTAssertNil(manager.restoreWorkspace(bookmarkSource: source))
        XCTAssertNil(manager.workspaceURL)
    }

    /// A corrupt bookmark file is dropped and restore returns nil.
    @MainActor
    func testRestoreWithCorruptBookmarkClearsFile() throws {
        let source = tempDir.appendingPathComponent("corrupt.bookmark")
        try Data("not a bookmark".utf8).write(to: source)

        let manager = WorkspaceManager()
        XCTAssertNil(manager.restoreWorkspace(bookmarkSource: source))
        XCTAssertFalse(FileManager.default.fileExists(atPath: source.path),
                       "corrupt bookmark should be removed")
        XCTAssertNil(manager.workspaceURL)
    }

    /// A valid persisted bookmark restores the workspace URL.
    @MainActor
    func testRestoreWithValidBookmarkReturnsURL() throws {
        let source = tempDir.appendingPathComponent("valid.bookmark")
        let manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))
        XCTAssertTrue(manager.persist(data, to: source))

        let restored = manager.restoreWorkspace(bookmarkSource: source)
        XCTAssertNotNil(restored)
        // Bookmark resolution may canonicalize the path (e.g. `/var` → `/private/var`).
        XCTAssertEqual(restored?.resolvingSymlinksInPath().path,
                       tempDir.resolvingSymlinksInPath().path)
        XCTAssertEqual(manager.workspaceURL, restored)
    }
}

// MARK: - T013 .muisti directory

extension WorkspaceBookmarkTests {

    /// No workspace selected → ensure fails without crashing.
    @MainActor
    func testEnsureMuistiDirectoryFailsWithoutWorkspace() {
        let manager = WorkspaceManager()
        XCTAssertFalse(manager.ensureMuistiDirectory())
    }

    /// Restoring a workspace creates the `.muisti` directory.
    @MainActor
    func testRestoreCreatesMuistiDirectory() throws {
        let source = tempDir.appendingPathComponent("workspace.bookmark")
        let manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))
        XCTAssertTrue(manager.persist(data, to: source))

        XCTAssertNotNil(manager.restoreWorkspace(bookmarkSource: source))
        let muistiDir = tempDir.appendingPathComponent(".muisti", isDirectory: true)
        var isDir: ObjCBool = false
        XCTAssertTrue(FileManager.default.fileExists(atPath: muistiDir.path, isDirectory: &isDir))
        XCTAssertTrue(isDir.boolValue, ".muisti should be a directory")
    }

    /// Restoring a workspace migrates the legacy `.minne` directory and keeps
    /// its contents intact.
    @MainActor
    func testRestoreMigratesLegacyMinneDirectory() throws {
        let legacy = tempDir.appendingPathComponent(".minne", isDirectory: true)
        try FileManager.default.createDirectory(at: legacy, withIntermediateDirectories: true)
        try Data("keep".utf8).write(to: legacy.appendingPathComponent("marker.txt"))

        let source = tempDir.appendingPathComponent("workspace.bookmark")
        let manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))
        XCTAssertTrue(manager.persist(data, to: source))

        XCTAssertNotNil(manager.restoreWorkspace(bookmarkSource: source))
        let current = tempDir.appendingPathComponent(".muisti", isDirectory: true)
        XCTAssertFalse(FileManager.default.fileExists(atPath: legacy.path))
        XCTAssertEqual(try String(contentsOf: current.appendingPathComponent("marker.txt")), "keep")
    }

    /// An existing `.muisti` directory is reused, not recreated.
    @MainActor
    func testEnsureReusesExistingMuistiDirectory() throws {
        let muistiDir = tempDir.appendingPathComponent(".muisti", isDirectory: true)
        try FileManager.default.createDirectory(at: muistiDir, withIntermediateDirectories: true)

        // Marker proving the directory was not wiped/recreated.
        let marker = muistiDir.appendingPathComponent("marker.txt")
        try Data("keep".utf8).write(to: marker)

        let source = tempDir.appendingPathComponent("workspace.bookmark")
        let manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))
        XCTAssertTrue(manager.persist(data, to: source))

        XCTAssertNotNil(manager.restoreWorkspace(bookmarkSource: source))
        XCTAssertTrue(manager.ensureMuistiDirectory())
        XCTAssertTrue(FileManager.default.fileExists(atPath: marker.path),
                      "existing .muisti content must be preserved")
    }
}
