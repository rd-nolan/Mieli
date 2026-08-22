import XCTest
@testable import Minne

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

// MARK: - T013 .minne directory

extension WorkspaceBookmarkTests {

    /// No workspace selected → ensure fails without crashing.
    @MainActor
    func testEnsureMinneDirectoryFailsWithoutWorkspace() {
        let manager = WorkspaceManager()
        XCTAssertFalse(manager.ensureMinneDirectory())
    }

    /// Restoring a workspace creates the `.minne` directory.
    @MainActor
    func testRestoreCreatesMinneDirectory() throws {
        let source = tempDir.appendingPathComponent("workspace.bookmark")
        let manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))
        XCTAssertTrue(manager.persist(data, to: source))

        XCTAssertNotNil(manager.restoreWorkspace(bookmarkSource: source))
        let minneDir = tempDir.appendingPathComponent(".minne", isDirectory: true)
        var isDir: ObjCBool = false
        XCTAssertTrue(FileManager.default.fileExists(atPath: minneDir.path, isDirectory: &isDir))
        XCTAssertTrue(isDir.boolValue, ".minne should be a directory")
    }

    /// An existing `.minne` directory is reused, not recreated.
    @MainActor
    func testEnsureReusesExistingMinneDirectory() throws {
        let minneDir = tempDir.appendingPathComponent(".minne", isDirectory: true)
        try FileManager.default.createDirectory(at: minneDir, withIntermediateDirectories: true)

        // Marker proving the directory was not wiped/recreated.
        let marker = minneDir.appendingPathComponent("marker.txt")
        try Data("keep".utf8).write(to: marker)

        let source = tempDir.appendingPathComponent("workspace.bookmark")
        let manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))
        XCTAssertTrue(manager.persist(data, to: source))

        XCTAssertNotNil(manager.restoreWorkspace(bookmarkSource: source))
        XCTAssertTrue(manager.ensureMinneDirectory())
        XCTAssertTrue(FileManager.default.fileExists(atPath: marker.path),
                      "existing .minne content must be preserved")
    }
}