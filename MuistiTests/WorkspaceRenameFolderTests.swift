import XCTest
@testable import Muisti

/// Verifies T024: renaming real folders on disk with path-safety guards.
final class WorkspaceRenameFolderTests: XCTestCase {

    private var bookmarkFile: URL!
    private var manager: WorkspaceManager!

    @MainActor
    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("muisti-renfold-\(UUID().uuidString)", isDirectory: true)
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

    @MainActor
    func testRenameFolder() {
        makeFolder("工作")
        XCTAssertTrue(manager.renameFolder(at: "工作", to: "归档"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作").path))
        var isDir: ObjCBool = false
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("归档").path, isDirectory: &isDir))
        XCTAssertTrue(isDir.boolValue)
    }

    @MainActor
    func testRenameNestedFolder() {
        makeFolder("工作/项目A")
        XCTAssertTrue(manager.renameFolder(at: "工作/项目A", to: "项目B"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目B").path))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目A").path))
    }

    @MainActor
    func testRenameKeepsContents() {
        makeFolder("工作")
        try? "# 内容\n".write(
            to: tempRoot.appendingPathComponent("工作/笔记.md"), atomically: true, encoding: .utf8)
        XCTAssertTrue(manager.renameFolder(at: "工作", to: "归档"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("归档/笔记.md").path))
    }

    @MainActor
    func testRejectsDestinationCollision() {
        makeFolder("工作")
        makeFolder("归档")
        XCTAssertFalse(manager.renameFolder(at: "工作", to: "归档"))
        // Both folders stay intact.
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("归档").path))
    }

    @MainActor
    func testRejectsPathSeparatorInNewName() {
        makeFolder("工作")
        XCTAssertFalse(manager.renameFolder(at: "工作", to: "归档/子"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作").path))
    }

    @MainActor
    func testRejectsEmptyOrDotNewName() {
        makeFolder("工作")
        XCTAssertFalse(manager.renameFolder(at: "工作", to: "  "))
        XCTAssertFalse(manager.renameFolder(at: "工作", to: ""))
        XCTAssertFalse(manager.renameFolder(at: "工作", to: "."))
        XCTAssertFalse(manager.renameFolder(at: "工作", to: ".."))
    }

    @MainActor
    func testRejectsInternalNewName() {
        makeFolder("工作")
        XCTAssertFalse(manager.renameFolder(at: "工作", to: ".muisti"))
        XCTAssertFalse(manager.renameFolder(at: "工作", to: "note.files"))
    }

    @MainActor
    func testRejectsRenameOfMissingOrNotePath() {
        // Missing folder.
        XCTAssertFalse(manager.renameFolder(at: "不存在的目录", to: "其他"))
        // A Markdown file is not a folder.
        try? "# 标题\n".write(
            to: tempRoot.appendingPathComponent("笔记.md"), atomically: true, encoding: .utf8)
        XCTAssertFalse(manager.renameFolder(at: "笔记.md", to: "其他"))
    }
}