import XCTest
@testable import Minne

/// Verifies T090: the watcher's pure snapshot/diff helpers and a live
/// polling smoke test.
final class WorkspaceWatcherTests: XCTestCase {

    private var dir: URL!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("minne-watcher-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: dir)
        dir = nil
    }

    private func write(_ rel: String, _ content: String = "x") throws {
        let url = dir.appendingPathComponent(rel)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(content.utf8).write(to: url)
    }

    func testIgnoresInternalAndAttachmentPaths() {
        XCTAssertTrue(WorkspaceWatcher.isIgnoredPath(".minne/index.sqlite"))
        XCTAssertTrue(WorkspaceWatcher.isIgnoredPath("a.files/pic.png"))
        XCTAssertTrue(WorkspaceWatcher.isIgnoredPath("工作/技术方案.files/api.pdf"))
        XCTAssertTrue(WorkspaceWatcher.isIgnoredPath("note.files"))
        XCTAssertFalse(WorkspaceWatcher.isIgnoredPath("工作/方案.md"))
        XCTAssertFalse(WorkspaceWatcher.isIgnoredPath("notes.md"))
        XCTAssertFalse(WorkspaceWatcher.isIgnoredPath("files.md"))
    }

    func testSnapshotOnlyCollectsMarkdown() throws {
        try write("a.md")
        try write("b.txt") // not markdown
        try write("图片.png")
        try write("工作/c.md")
        try write(".minne/idx.sqlite") // ignored dir
        try write("d.files/pic.png")   // ignored attachment folder

        let snap = try XCTUnwrap(WorkspaceWatcher.workspaceSnapshot(root: dir))
        XCTAssertEqual(Set(snap.keys), ["a.md", "工作/c.md"])
    }

    func testDiffReportsCreateModifyDelete() throws {
        let stamp = WorkspaceWatcher.FileStamp(size: 1, mtime: 1)
        let changedStamp = WorkspaceWatcher.FileStamp(size: 2, mtime: 5)

        let changes = WorkspaceWatcher.diff(
            new: ["a.md": stamp, "b.md": changedStamp],
            old: ["b.md": stamp, "c.md": stamp])

        XCTAssertEqual(Set(changes.map { String(describing: $0.kind) }),
                       Set(["created", "modified", "deleted"]))
        XCTAssertTrue(changes.contains { $0.path == "a.md" && $0.kind == .created })
        XCTAssertTrue(changes.contains { $0.path == "b.md" && $0.kind == .modified })
        XCTAssertTrue(changes.contains { $0.path == "c.md" && $0.kind == .deleted })
    }

    func testSnapshotDiffUnchangedIsEmpty() throws {
        let stamp = WorkspaceWatcher.FileStamp(size: 1, mtime: 1)
        let s = ["a.md": stamp]
        XCTAssertTrue(WorkspaceWatcher.diff(new: s, old: s).isEmpty)
    }

    /// T109: a rewrite within the same second (same size) must still be
    /// detected via the sub-second mtime, matching the indexer's precision.
    func testDiffDetectsSameSecondSubSecondModification() throws {
        let before = WorkspaceWatcher.FileStamp(size: 100, mtime: 1_700_000_000.25)
        // Same size, same whole-second second, only fractional part differs.
        let after = WorkspaceWatcher.FileStamp(size: 100, mtime: 1_700_000_000.75)

        let changes = WorkspaceWatcher.diff(new: ["a.md": after], old: ["a.md": before])
        XCTAssertEqual(changes.count, 1)
        XCTAssertEqual(changes[0].path, "a.md")
        XCTAssertEqual(changes[0].kind, .modified)
    }

    // MARK: live polling smoke (real filesystem, real watcher loop)

    func testPollingWatcherReportsCreatedNote() throws {
        let watcher = WorkspaceWatcher()
        watcher.interval = 0.3
        let got = expectation(description: "created note observed")
        watcher.onChanges = { changes in
            if changes.contains(where: { $0.path == "新.md" && $0.kind == .created }) {
                got.fulfill()
            }
        }
        watcher.start(root: dir)
        // Let the baseline scan settle, then mutate.
        Thread.sleep(forTimeInterval: 0.6)
        try Data("内容".utf8).write(to: dir.appendingPathComponent("新.md"))

        wait(for: [got], timeout: 4)
        watcher.stop()
    }

    func testStoppedWatcherDoesNotPoll() throws {
        let watcher = WorkspaceWatcher()
        watcher.interval = 0.1
        var calls = 0
        watcher.onChanges = { _ in calls += 1 }
        watcher.start(root: dir)
        Thread.sleep(forTimeInterval: 0.35)
        watcher.stop()
        Thread.sleep(forTimeInterval: 0.4)
        XCTAssertEqual(calls, 0)
    }
}