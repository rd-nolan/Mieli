import XCTest
@testable import Muisti

/// Verifies T026: moving Markdown notes between folders within the workspace.
final class WorkspaceMoveNoteTests: XCTestCase {

    private var bookmarkFile: URL!
    private var manager: WorkspaceManager!

    @MainActor
    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("muisti-movenote-\(UUID().uuidString)", isDirectory: true)
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

    private func makeFolder(_ rel: String) {
        try? FileManager.default.createDirectory(
            at: tempRoot.appendingPathComponent(rel), withIntermediateDirectories: true)
    }

    @MainActor
    func testMoveNoteIntoFolder() {
        makeFolder("归档")
        makeNote("笔记.md")
        XCTAssertTrue(manager.moveNote(at: "笔记.md", toDirectory: "归档"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("笔记.md").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("归档/笔记.md").path))
    }

    @MainActor
    func testMoveNoteToRoot() {
        makeFolder("工作")
        makeNote("工作/笔记.md")
        XCTAssertTrue(manager.moveNote(at: "工作/笔记.md", toDirectory: ""))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/笔记.md").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("笔记.md").path))
    }

    @MainActor
    func testMoveNestedNoteIntoNestedFolder() {
        makeFolder("工作/项目A")
        makeNote("临时/笔记.md")
        XCTAssertTrue(manager.moveNote(at: "临时/笔记.md", toDirectory: "工作/项目A"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目A/笔记.md").path))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("临时/笔记.md").path))
    }

    @MainActor
    func testMovePreservesContent() {
        makeFolder("归档")
        makeNote("笔记.md", content: "# 特殊内容\n行2\n")
        XCTAssertTrue(manager.moveNote(at: "笔记.md", toDirectory: "归档"))
        let content = try? String(
            contentsOf: tempRoot.appendingPathComponent("归档/笔记.md"), encoding: .utf8)
        XCTAssertEqual(content, "# 特殊内容\n行2\n")
    }

    @MainActor
    func testRejectsDestinationCollision() {
        makeFolder("归档")
        makeNote("笔记.md")
        makeNote("归档/笔记.md")
        XCTAssertFalse(manager.moveNote(at: "笔记.md", toDirectory: "归档"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("笔记.md").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("归档/笔记.md").path))
    }

    @MainActor
    func testRejectsMissingTargetFolder() {
        makeNote("笔记.md")
        XCTAssertFalse(manager.moveNote(at: "笔记.md", toDirectory: "不存在的目录"))
    }

    @MainActor
    func testRejectsInternalTargetFolder() {
        makeNote("笔记.md")
        XCTAssertFalse(manager.moveNote(at: "笔记.md", toDirectory: ".muisti"))
        XCTAssertFalse(manager.moveNote(at: "笔记.md", toDirectory: "note.files"))
    }

    @MainActor
    func testRejectsMissingSourceOrFolderSource() {
        XCTAssertFalse(manager.moveNote(at: "不存在.md", toDirectory: ""))
        makeFolder("目录")
        XCTAssertFalse(manager.moveNote(at: "目录", toDirectory: ""))
    }

    @MainActor
    func testRelativeWorkspacePath() {
        makeFolder("工作/子")
        let root = manager.workspaceURL!
        XCTAssertEqual(manager.relativeWorkspacePath(of: root), "")
        XCTAssertEqual(manager.relativeWorkspacePath(
            of: tempRoot.appendingPathComponent("工作")), "工作")
        XCTAssertEqual(manager.relativeWorkspacePath(
            of: tempRoot.appendingPathComponent("工作/子")), "工作/子")
        // Outside the workspace.
        XCTAssertNil(manager.relativeWorkspacePath(
            of: tempRoot.deletingLastPathComponent()))
    }
}