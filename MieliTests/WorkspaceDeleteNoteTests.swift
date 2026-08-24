import XCTest
@testable import Mieli

/// Verifies T027: permanently deleting Markdown note files.
/// Deletion UI confirmation is a View concern; here we verify the model action.
final class WorkspaceDeleteNoteTests: XCTestCase {

    private var bookmarkFile: URL!
    private var manager: WorkspaceManager!

    @MainActor
    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mieli-delnote-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        bookmarkFile = tempDir.appendingPathComponent("ws.bookmark")

        manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))
        XCTAssertTrue(manager.persist(data, to: bookmarkFile))
        XCTAssertNotNil(manager.restoreWorkspace(bookmarkSource: bookmarkFile))
        tempRoot = tempDir
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: tempDir)
        tempDir = nil
        tempRoot = nil
        manager = nil
    }

    private var tempDir: URL!
    private var tempRoot: URL!

    private func makeNote(_ rel: String, content: String = "# 标题\n") {
        let url = tempRoot.appendingPathComponent(rel)
        try? FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? content.write(to: url, atomically: true, encoding: .utf8)
    }

    @MainActor
    func testDeleteNoteFile() {
        makeNote("周报.md")
        XCTAssertTrue(manager.deleteNoteFile(at: "周报.md"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("周报.md").path))
    }

    @MainActor
    func testDeleteNestedNote() {
        makeNote("工作/项目A/技术方案.md")
        XCTAssertTrue(manager.deleteNoteFile(at: "工作/项目A/技术方案.md"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目A/技术方案.md").path))
        // Parent folders remain.
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目A").path))
    }

    @MainActor
    func testDeleteOnlyRemovesTargetFile() {
        makeNote("笔记.md")
        makeNote("其他.md")
        XCTAssertTrue(manager.deleteNoteFile(at: "笔记.md"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("其他.md").path))
    }

    @MainActor
    func testRejectsDeletingFolder() {
        try? FileManager.default.createDirectory(
            at: tempRoot.appendingPathComponent("目录"), withIntermediateDirectories: true)
        XCTAssertFalse(manager.deleteNoteFile(at: "目录"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("目录").path))
    }

    @MainActor
    func testRejectsNonMarkdownFile() {
        makeNote("照片.jpg")
        XCTAssertFalse(manager.deleteNoteFile(at: "照片.jpg"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("照片.jpg").path))
    }

    @MainActor
    func testRejectsMissingSource() {
        XCTAssertFalse(manager.deleteNoteFile(at: "不存在.md"))
    }

    @MainActor
    func testSecondDeleteFails() {
        makeNote("笔记.md")
        XCTAssertTrue(manager.deleteNoteFile(at: "笔记.md"))
        XCTAssertFalse(manager.deleteNoteFile(at: "笔记.md"))
    }

    @MainActor
    func testRejectsInternalAndTraversalPaths() {
        // Nearest internal path is rejected before touching disk.
        XCTAssertFalse(manager.deleteNoteFile(at: ".mieli/数据.md"))
        XCTAssertFalse(manager.deleteNoteFile(at: "../outside.md"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.deletingLastPathComponent().appendingPathComponent("outside.md").path))
    }
}