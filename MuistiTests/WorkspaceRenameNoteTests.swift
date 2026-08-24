import XCTest
@testable import Muisti

/// Verifies T025: renaming Markdown note files on disk.
/// Attachment (`.files`) renaming is a later task (T086) and not covered here.
final class WorkspaceRenameNoteTests: XCTestCase {

    private var bookmarkFile: URL!
    private var manager: WorkspaceManager!

    @MainActor
    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("muisti-rennote-\(UUID().uuidString)", isDirectory: true)
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
    func testRenameNoteAppendsExtension() {
        makeNote("周报.md")
        XCTAssertTrue(manager.renameNoteFile(at: "周报.md", to: "月报"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("周报.md").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("月报.md").path))
    }

    @MainActor
    func testRenameNoteKeepsExplicitMdExtension() {
        makeNote("需求分析.md")
        XCTAssertTrue(manager.renameNoteFile(at: "需求分析.md", to: "技术方案.md"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("技术方案.md").path))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("需求分析.md").path))
    }

    @MainActor
    func testRenameNestedNoteStaysInParent() {
        makeNote("工作/项目A/笔记.md")
        XCTAssertTrue(manager.renameNoteFile(at: "工作/项目A/笔记.md", to: "纪要"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目A/纪要.md").path))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目A/笔记.md").path))
    }

    @MainActor
    func testRenamePreservesContent() {
        makeNote("原始.md", content: "# 重要内容\n第二行\n")
        XCTAssertTrue(manager.renameNoteFile(at: "原始.md", to: "新名称"))
        let content = try? String(contentsOf: tempRoot.appendingPathComponent("新名称.md"),
                                  encoding: .utf8)
        XCTAssertEqual(content, "# 重要内容\n第二行\n")
    }

    @MainActor
    func testRejectsDestinationCollision() {
        makeNote("甲.md")
        makeNote("乙.md")
        XCTAssertFalse(manager.renameNoteFile(at: "甲.md", to: "乙"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("甲.md").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("乙.md").path))
    }

    @MainActor
    func testRejectsNonMarkdownExtension() {
        makeNote("照片.md")
        XCTAssertFalse(manager.renameNoteFile(at: "照片.md", to: "照片.jpg"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("照片.md").path))
    }

    @MainActor
    func testRejectsPathSeparatorAndEmpty() {
        makeNote("笔记.md")
        XCTAssertFalse(manager.renameNoteFile(at: "笔记.md", to: "子/笔记"))
        XCTAssertFalse(manager.renameNoteFile(at: "笔记.md", to: "  "))
        XCTAssertFalse(manager.renameNoteFile(at: "笔记.md", to: ""))
        XCTAssertFalse(manager.renameNoteFile(at: "笔记.md", to: "."))
    }

    @MainActor
    func testRejectsMissingSourceOrFolder() {
        XCTAssertFalse(manager.renameNoteFile(at: "不存在.md", to: "其他"))
        // A folder is not a note.
        try? FileManager.default.createDirectory(
            at: tempRoot.appendingPathComponent("目录"), withIntermediateDirectories: true)
        XCTAssertFalse(manager.renameNoteFile(at: "目录", to: "别的"))
    }

    @MainActor
    func testRejectsPathTraversal() {
        XCTAssertFalse(manager.renameNoteFile(at: "../outside", to: "x"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.deletingLastPathComponent().appendingPathComponent("x.md").path))
    }

    // MARK: T086 — renaming a note also renames its `.files` folder and links

    private func makeAttachments(_ stem: String) {
        let url = tempRoot.appendingPathComponent("\(stem).files")
        try? FileManager.default.createDirectory(
            at: url, withIntermediateDirectories: true)
        try? "png".data(using: .utf8)?.write(to: url.appendingPathComponent("pic.png"))
    }

    @MainActor
    func testRenameNoteMovesAttachmentFolderAndRewritesLinks() {
        makeNote("旧名.md", content: "# 旧\n![图](./旧名.files/pic.png)\n")
        makeAttachments("旧名")

        XCTAssertTrue(manager.renameNote(at: "旧名.md", to: "新名"))

        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("旧名.md").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("新名.md").path))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("旧名.files").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("新名.files/pic.png").path))
        let content = try? String(contentsOf: tempRoot.appendingPathComponent("新名.md"),
                                  encoding: .utf8)
        XCTAssertEqual(content, "# 旧\n![图](./新名.files/pic.png)\n")
    }

    @MainActor
    func testRenameNestedNoteRewritesLinksInPlace() {
        makeNote("工作/方案.md",
                 content: "# 方案\n[PDF](./方案.files/api.pdf)\n")
        makeAttachments("工作/方案")

        XCTAssertTrue(manager.renameNote(at: "工作/方案.md", to: "定稿"))

        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/定稿.files/pic.png").path))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/方案.files/pic.png").path))
        let content = try? String(contentsOf: tempRoot.appendingPathComponent("工作/定稿.md"),
                                  encoding: .utf8)
        XCTAssertEqual(content, "# 方案\n[PDF](./定稿.files/api.pdf)\n")
    }

    @MainActor
    func testRenameNoteWithoutAttachmentsIsNoOpForFolder() {
        makeNote("纯文本.md", content: "# 无附件\n正文。\n")
        XCTAssertTrue(manager.renameNote(at: "纯文本.md", to: "纯文本2"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("纯文本.files").path))
        let content = try? String(contentsOf: tempRoot.appendingPathComponent("纯文本2.md"),
                                  encoding: .utf8)
        XCTAssertEqual(content, "# 无附件\n正文。\n")
    }

    @MainActor
    func testRewritingAttachmentLinksIsPureAndSelective() {
        let md = "![a](./旧名.files/a.png)\n[link](旧名.files/x.pdf)\n### 旧名.files 不是路径\n"
        let out = AttachmentService.rewritingAttachmentLinks(md, oldStem: "旧名", newStem: "新名")
        XCTAssertTrue(out.contains("./新名.files/a.png"))
        XCTAssertTrue(out.contains("新名.files/x.pdf"))
        XCTAssertFalse(out.contains("旧名.files/")) // path prefixes rewritten
        XCTAssertTrue(out.contains("### 旧名.files 不是路径")) // unrelated kept
    }

    @MainActor
    func testRenameAttachmentFolderCollisionReturnsFalse() {
        makeAttachments("甲")
        makeAttachments("乙")
        XCTAssertFalse(AttachmentService.renameAttachmentFolder(
            fromNoteStem: "甲", toNoteStem: "乙", in: tempRoot))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("甲.files/pic.png").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("乙.files/pic.png").path))
    }
}