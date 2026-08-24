import XCTest
@testable import Muisti

/// Verifies T023: creating real Markdown notes on disk with path-safety guards.
final class WorkspaceCreateNoteTests: XCTestCase {

    private var bookmarkFile: URL!
    private var manager: WorkspaceManager!

    @MainActor
    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("muisti-note-\(UUID().uuidString)", isDirectory: true)
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

    @MainActor
    func testCreateNoteAtRootAppendsExtension() {
        XCTAssertTrue(manager.createNote(at: "周报"))
        let file = tempRoot.appendingPathComponent("周报.md")
        XCTAssertTrue(FileManager.default.fileExists(atPath: file.path))
        let content = try? String(contentsOf: file, encoding: .utf8)
        // T032: a new note carries full Front Matter plus an H1 title.
        let fm = FrontMatterParser.parse(content ?? "")
        XCTAssertNotNil(fm)
        XCTAssertEqual(fm?.id?.count, 26)
        XCTAssertEqual(fm?.tags, [])
        XCTAssertNotNil(fm?.created)
        XCTAssertTrue(content?.hasSuffix("---\n") ?? false)
    }

    @MainActor
    func testCreateDefaultNoteUsesDefaultNameAndAvoidsCollision() {
        XCTAssertEqual(manager.createDefaultNote(in: nil), "新建笔记.md")
        XCTAssertEqual(manager.createDefaultNote(in: nil), "新建笔记 2.md")
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("新建笔记.md").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("新建笔记 2.md").path))
    }

    func testRenameRequestIsRejectedAfterWorkspaceChanges() {
        let original = URL(fileURLWithPath: "/tmp/muisti-original", isDirectory: true)
        let replacement = URL(fileURLWithPath: "/tmp/muisti-replacement", isDirectory: true)

        XCTAssertFalse(ContentView.isRenameRequestCurrent(
            originWorkspaceURL: original,
            currentWorkspaceURL: replacement
        ))
    }

    func testRenameRequestRemainsCurrentInOriginWorkspace() {
        let workspace = URL(fileURLWithPath: "/tmp/muisti-workspace", isDirectory: true)

        XCTAssertTrue(ContentView.isRenameRequestCurrent(
            originWorkspaceURL: workspace,
            currentWorkspaceURL: workspace
        ))
    }

    @MainActor
    func testCreateNoteKeepsExplicitMdExtension() {
        XCTAssertTrue(manager.createNote(at: "需求分析.md"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("需求分析.md").path))
    }

    @MainActor
    func testCreateNestedNoteCreatesParentFolders() {
        XCTAssertTrue(manager.createNote(at: "工作/项目A/技术方案"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目A/技术方案.md").path))
    }

    @MainActor
    func testDoesNotOverwriteExistingNote() {
        let note = tempRoot.appendingPathComponent("已有.md")
        try? "# 原有内容\n".write(to: note, atomically: true, encoding: .utf8)
        XCTAssertFalse(manager.createNote(at: "已有"))
        let content = try? String(contentsOf: note, encoding: .utf8)
        XCTAssertEqual(content, "# 原有内容\n")
    }

    @MainActor
    func testRejectsTraversal() {
        XCTAssertFalse(manager.createNote(at: "../outside"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.deletingLastPathComponent().appendingPathComponent("outside.md").path))
    }

    @MainActor
    func testRejectsAbsolutePath() {
        XCTAssertFalse(manager.createNote(at: "/tmp/elsewhere"))
    }

    @MainActor
    func testRejectsEmptyName() {
        XCTAssertFalse(manager.createNote(at: "   "))
        XCTAssertFalse(manager.createNote(at: ""))
    }

    @MainActor
    func testRejectsInternalDirectories() {
        XCTAssertFalse(manager.createNote(at: ".muisti/data"))
        XCTAssertFalse(manager.createNote(at: "note.files/sub"))
    }

    @MainActor
    func testRejectsNonMarkdownExtension() {
        XCTAssertFalse(manager.createNote(at: "照片.jpg"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("照片.jpg").path))
    }

    // MARK: - T083 image drop

    @MainActor
    func testAddImageAttachmentCopiesAndBuildsFragment() throws {
        XCTAssertTrue(manager.createNote(at: "工作/技术方案"))
        let source = tempRoot.appendingPathComponent("arch.png")
        try Data("img-bytes".utf8).write(to: source)

let fragment = manager.addImageAttachment(from: source.path, forNoteAt: "工作/技术方案.md")
        XCTAssertNotNil(fragment)
        XCTAssertEqual(fragment, "![arch.png](./技术方案.files/arch.png)")

        // File copied into the note's .files folder.
        let folder = tempRoot.appendingPathComponent("工作/技术方案.files")
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: folder.appendingPathComponent("arch.png").path))
        XCTAssertEqual(
            try String(contentsOf: folder.appendingPathComponent("arch.png"), encoding: .utf8),
            "img-bytes")
    }

    @MainActor
    func testAddImageAttachmentUniquesDuplicateName() throws {
        XCTAssertTrue(manager.createNote(at: "笔记"))
        let source = tempRoot.appendingPathComponent("img-x.png")
        try? Data("a".utf8).write(to: source)
        _ = manager.addImageAttachment(from: source.path, forNoteAt: "笔记.md")
        // Second drop of the same name → -1 suffix.
        let fragment2 = manager.addImageAttachment(from: source.path, forNoteAt: "笔记.md")
        XCTAssertEqual(fragment2, "![img-x-1.png](./笔记.files/img-x-1.png)")
    }

    @MainActor
    func testAddImageAttachmentNilWhenSourceMissing() {
        XCTAssertTrue(manager.createNote(at: "笔记"))
        let fragment = manager.addImageAttachment(
            from: tempRoot.appendingPathComponent("不存在.png").path, forNoteAt: "笔记.md")
        XCTAssertNil(fragment)
    }

    // MARK: - T084 generic file drop

    @MainActor
    func testAddAttachmentLinkBuildsLinkFragment() throws {
        XCTAssertTrue(manager.createNote(at: "资料"))
        let source = tempRoot.appendingPathComponent("手册.pdf")
        try Data("%PDF-bytes".utf8).write(to: source)

        let fragment = manager.addAttachmentLink(from: source.path, forNoteAt: "资料.md")
        XCTAssertEqual(fragment, "[手册.pdf](./资料.files/手册.pdf)")

        let folder = tempRoot.appendingPathComponent("资料.files")
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: folder.appendingPathComponent("手册.pdf").path))
    }

    @MainActor
    func testAddAttachmentLinkForZipAndUniqueName() throws {
        XCTAssertTrue(manager.createNote(at: "附件"))
        let src = tempRoot.appendingPathComponent("bundle.zip")
        try? Data("z".utf8).write(to: src)
        _ = manager.addAttachmentLink(from: src.path, forNoteAt: "附件.md")
        let second = manager.addAttachmentLink(from: src.path, forNoteAt: "附件.md")
        XCTAssertEqual(second, "[bundle-1.zip](./附件.files/bundle-1.zip)")
    }

    @MainActor
    func testAddAttachmentLinkNilWhenSourceMissing() {
        XCTAssertTrue(manager.createNote(at: "附件"))
        let fragment = manager.addAttachmentLink(
            from: tempRoot.appendingPathComponent("缺失.txt").path, forNoteAt: "附件.md")
        XCTAssertNil(fragment)
    }
}
