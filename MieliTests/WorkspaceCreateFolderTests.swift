import XCTest
import Testing
import AppKit
@testable import Mieli

/// Verifies T022: creating real folders on disk, with path-safety guards.
final class WorkspaceCreateFolderTests: XCTestCase {

    private var bookmarkFile: URL!
    private var manager: WorkspaceManager!

    @MainActor
    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mieli-folder-\(UUID().uuidString)", isDirectory: true)
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
    func testCreateFolderAtRoot() {
        XCTAssertTrue(manager.createFolder(at: "项目"))
        var isDir: ObjCBool = false
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("项目").path, isDirectory: &isDir))
        XCTAssertTrue(isDir.boolValue)
    }

    @MainActor
    func testCreateDefaultFolderUsesCategoryNameAndAvoidsCollision() {
        XCTAssertEqual(manager.createDefaultFolder(in: nil), "新建分类")
        XCTAssertEqual(manager.createDefaultFolder(in: nil), "新建分类 2")
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("新建分类").path))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("新建分类 2").path))
    }

    @MainActor
    func testCreateNestedFolderCreatesParents() {
        XCTAssertTrue(manager.createFolder(at: "工作/项目A/子目录"))
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("工作/项目A/子目录").path))
    }

    @MainActor
    func testRejectsTraversal() {
        XCTAssertFalse(manager.createFolder(at: "../outside"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.deletingLastPathComponent().appendingPathComponent("outside").path))
    }

    @MainActor
    func testRejectsAbsolutePath() {
        XCTAssertFalse(manager.createFolder(at: "/tmp/elsewhere"))
    }

    @MainActor
    func testRejectsEmptyName() {
        XCTAssertFalse(manager.createFolder(at: "   "))
        XCTAssertFalse(manager.createFolder(at: ""))
    }

    @MainActor
    func testRejectsInternalDirectories() {
        XCTAssertFalse(manager.createFolder(at: ".mieli/data"))
        XCTAssertFalse(manager.createFolder(at: "note.files/sub"))
        XCTAssertFalse(FileManager.default.fileExists(
            atPath: tempRoot.appendingPathComponent("note.files").path))
    }

    @MainActor
    func testRejectsDotSegments() {
        XCTAssertFalse(manager.createFolder(at: "a/./b"))
        XCTAssertFalse(manager.createFolder(at: "a/../b"))
    }
}

@Suite("T127 sidebar scroller styling")
struct SidebarScrollerStyleTests {
    @Test("uses a small auto-hiding overlay scroller")
    @MainActor
    func smallOverlayScroller() {
        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true

        SidebarScrollerConfigurator.configure(scrollView)

        #expect(scrollView.scrollerStyle == .overlay)
        #expect(scrollView.autohidesScrollers)
        #expect(scrollView.verticalScroller?.controlSize == .small)
    }
}
