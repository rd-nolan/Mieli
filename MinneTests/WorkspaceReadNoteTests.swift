import XCTest
@testable import Minne

/// Verifies T062: reading a workspace Markdown note's content via
/// `WorkspaceManager.readNote(at:)`.
final class WorkspaceReadNoteTests: XCTestCase {

    private var tempDir: URL!
    private var bookmarkFile: URL!
    private var manager: WorkspaceManager!

    @MainActor
    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("minne-readnote-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        bookmarkFile = tempDir.appendingPathComponent("ws.bookmark")

        manager = WorkspaceManager()
        let data = try XCTUnwrap(manager.createBookmark(for: tempDir))
        XCTAssertTrue(manager.persist(data, to: bookmarkFile))
        XCTAssertNotNil(manager.restoreWorkspace(bookmarkSource: bookmarkFile))
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: tempDir)
        tempDir = nil
        bookmarkFile = nil
        manager = nil
    }

    @MainActor
    func testReadExistingNoteReturnsContent() {
        XCTAssertTrue(manager.createNote(at: "A"))
        let content = manager.readNote(at: "A.md")
        // createNote writes Front Matter + a title heading.
        XCTAssertNotNil(content)
        XCTAssertTrue(content?.contains("# A") == true)
        XCTAssertTrue(content?.hasPrefix("---") == true)
    }

    @MainActor
    func testReadNestedNote() throws {
        try FileManager.default.createDirectory(
            at: tempDir.appendingPathComponent("项目A"),
            withIntermediateDirectories: true)
        XCTAssertTrue(manager.createNote(at: "项目A/笔记"))
        let content = manager.readNote(at: "项目A/笔记.md")
        XCTAssertNotNil(content)
        XCTAssertTrue(content?.contains("# 笔记") == true)
    }

    @MainActor
    func testReadMissingNoteReturnsNil() {
        XCTAssertNil(manager.readNote(at: "不存在.md"))
        XCTAssertNil(manager.readNote(at: "项目/不存在.md"))
    }

    @MainActor
    func testRejectsNonMarkdownPaths() throws {
        // A .txt note path must be rejected.
        XCTAssertNil(manager.readNote(at: "foo.txt"))
        // Path traversal / internal names must be rejected.
        XCTAssertNil(manager.readNote(at: "../outside.md"))
        XCTAssertNil(manager.readNote(at: ".minne.md"))
    }

    @MainActor
    func testEditedContentSavesAndReadsBack() throws {
        // T064: edited Markdown persists via FileService and is re-readable.
        XCTAssertTrue(manager.createNote(at: "B"))
        let url = tempDir.appendingPathComponent("B.md")
        let edited = "# 新标题\n\n编辑后的正文。\n"
        XCTAssertTrue(try FileService.saveMarkdown(edited, to: url))
        // The manager reads back exactly what was saved (atomic write kept it intact).
        XCTAssertEqual(manager.readNote(at: "B.md"), edited)
    }

    @MainActor
    func testTagsParsedFromFrontMatter() throws {
        let md = """
        ---
        id: t1
        tags:
          - Swift
          - macOS
        ---
        # 标题
        """
        try md.write(to: tempDir.appendingPathComponent("Tag.md"), atomically: true, encoding: .utf8)
        XCTAssertEqual(manager.tags(forNoteAt: "Tag.md"), ["Swift", "macOS"])
    }

    @MainActor
    func testTagsEmptyWhenNoFrontMatter() throws {
        try "# 无标签\n".write(to: tempDir.appendingPathComponent("No.md"), atomically: true, encoding: .utf8)
        XCTAssertEqual(manager.tags(forNoteAt: "No.md"), [])
        XCTAssertEqual(manager.tags(forNoteAt: "missing.md"), [])
    }
}