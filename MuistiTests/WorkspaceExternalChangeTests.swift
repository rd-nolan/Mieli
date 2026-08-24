import XCTest
@testable import Muisti

/// E2E regression for the "external modification while editing" warning (T095).
///
/// The user reported a spurious "外部修改笔记" alert during normal editing: the
/// watcher is poll-based, so the app's own atomic autosave is observed as a
/// `.modified` on a *later* scan — a point where the old `isSelfWrite` flag
/// (cleared right after the synchronous write) had already reset, so the echo
/// was misread as *external*. The fix (T095) records the on-disk stamp the app
/// wrote and suppresses a `.modified` whose stamp matches; a genuinely external
/// write still fires the conflict.
final class WorkspaceExternalChangeTests: XCTestCase {

    private var tempDir: URL!
    private var bookmarkFile: URL!
    private var manager: WorkspaceManager!

    override func tearDownWithError() throws {
        manager = nil
        try? FileManager.default.removeItem(at: tempDir)
        tempDir = nil
        bookmarkFile = nil
    }

    /// Boots a real WorkspaceManager bound to a fresh temp workspace (the same
    /// bookmark-in-the-same-process flow the app's `restoreWorkspace` uses),
    /// with the real poll-based watcher started.
    @MainActor
    private func makeManager(noteContent: String) throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("muisti-ext-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        bookmarkFile = tempDir.appendingPathComponent("ws.bookmark")

        // Seed a note before restore so the watcher's baseline snapshot includes it.
        try noteContent.write(
            to: tempDir.appendingPathComponent("note.md"), atomically: true, encoding: .utf8)

        manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))
        XCTAssertTrue(manager.persist(data, to: bookmarkFile))
        XCTAssertNotNil(manager.restoreWorkspace(bookmarkSource: bookmarkFile))
        // The note is the "open" one for T095.
        manager.openedNotePath = "note.md"
    }

    /// The app's own atomic autosave must NOT surface as an external change:
    /// `recordSelfWrite` marks the on-disk stamp so the delayed `.modified`
    /// echo from the poll-based watcher is suppressed.
    @MainActor
    func testOwnAutosaveDoesNotTriggerConflict() async throws {
        try makeManager(noteContent: "# v1\n\n正文一" )
        let before = manager.externalEventID
        manager.openedNotePath = "note.md"

        // App writes the note via the atomic save, then records its stamp.
        let url = manager.workspaceURL!.appendingPathComponent("note.md")
        _ = try FileService.saveMarkdown("# v1\n\n正文一(edited)", to: url)
        manager.recordSelfWrite(at: "note.md")

        // Let the watcher poll a few times; the own echo must be suppressed.
        try await Task.sleep(for: .seconds(2.5))
        XCTAssertEqual(manager.externalEventID, before,
                       "app's own autosave must not be flagged as an external change")
    }

    /// A genuine edit from another program (different content, no
    /// `recordSelfWrite`) must NOT be swallowed — the conflict still fires.
    @MainActor
    func testGenuineExternalWriteFiresConflict() async throws {
        try makeManager(noteContent: "# v1\n\n原内容")
        let before = manager.externalEventID

        // Some other program rewrites the note (different stamp, no self-write).
        let url = manager.workspaceURL!.appendingPathComponent("note.md")
        try "# v1\n\n外部程序改的内容".write(to: url, atomically: true, encoding: .utf8)

        // Wait for the watcher's poll to observe the external modification.
        try await Task.sleep(for: .seconds(2.5))
        XCTAssertGreaterThan(manager.externalEventID, before,
                             "a true external write must still flag the open note as changed")
    }
}