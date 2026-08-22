import XCTest
@testable import Minne

/// Verifies T028: deleting folders, with explicit warnings for non-empty ones.
/// Recursive delete is confirmed by the caller (the UI) before being invoked.
final class WorkspaceDeleteFolderTests: XCTestCase {

    private var bookmarkFile: URL!
    private var manager: WorkspaceManager!

    @MainActor
    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("minne-delfld-\(UUID().uuidString)", isDirectory: true)
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

    private func makeFolder(_ rel: String) {
        try? FileManager.default.createDirectory(
            at: tempRoot.appendingPathComponent(rel), withIntermediateDirectories: true)
    }

    private func makeFile(_ rel: String) {
        try? "# 内容\n".write(
            to: tempRoot.appendingPathComponent(rel), atomically: true, encoding: .utf8)
    }

    @MainActor
    func testDeleteEmptyFolder() {
        makeFolder("空目录")
        XCTAssertEqual(manager.folderItemCount(for: "空目录"), 0)
        XCTAssertTrue(manager.deleteFolder(at: "空目录"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("空目录").path))
    }

    @MainActor
    func testDeleteNonEmptyFolderRecursively() {
        makeFolder("工作/子")
        makeFile("工作/笔记.md")
        makeFile("工作/子/说明.txt")
        XCTAssertEqual(manager.folderItemCount(for: "工作"), 2)
        XCTAssertTrue(manager.deleteFolder(at: "工作"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作").path))
        // Sibling folders remain untouched.
        makeFolder("归档")
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("归档").path))
    }

    @MainActor
    func testDeleteNestedFolderKeepsParent() {
        makeFolder("工作/项目A")
        makeFile("工作/项目A/技术方案.md")
        XCTAssertTrue(manager.deleteFolder(at: "工作/项目A"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目A").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作").path))
    }

    @MainActor
    func testFolderItemCountCountsTopLevelEntries() {
        makeFolder("混合/内")
        makeFile("混合/a.md")
        makeFile("混合/b.md")
        XCTAssertEqual(manager.folderItemCount(for: "混合"), 3)
    }

    @MainActor
    func testFolderItemCountNilForFileOrMissing() {
        makeFile("笔记.md")
        XCTAssertNil(manager.folderItemCount(for: "笔记.md"))
        XCTAssertNil(manager.folderItemCount(for: "不存在"))
    }

    @MainActor
    func testRejectsDeletingFileViaDeleteFolder() {
        makeFile("笔记.md")
        XCTAssertFalse(manager.deleteFolder(at: "笔记.md"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("笔记.md").path))
    }

    @MainActor
    func testRejectsMissingFolder() {
        XCTAssertFalse(manager.deleteFolder(at: "不存在"))
    }

    @MainActor
    func testRejectsInternalAndTraversalPaths() {
        XCTAssertFalse(manager.deleteFolder(at: ".minne"))
        // The internal directory must be left untouched (never deleted).
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent(".minne").path))
        XCTAssertFalse(manager.deleteFolder(at: "../outside"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.deletingLastPathComponent().appendingPathComponent("outside").path))
    }
}