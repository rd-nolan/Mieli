import XCTest
@testable import Mieli

/// Verifies T020: recursive Workspace scanning recognizes directories and
    /// Markdown notes, ignores internal directories + `*.files`, and reports relative paths.
final class WorkspaceScannerTests: XCTestCase {

    private var root: URL!

    override func setUpWithError() throws {
        root = FileManager.default.temporaryDirectory
            .appendingPathComponent("mieli-scan-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: root)
        root = nil
    }

    private func makeFile(relative: String) throws {
        let url = root.appendingPathComponent(relative)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("# placeholder".utf8).write(to: url)
    }

    private func makeDir(_ relative: String) throws {
        try FileManager.default.createDirectory(
            at: root.appendingPathComponent(relative), withIntermediateDirectories: true)
    }

    /// Scans markdown files, nested folders, and relative paths; ignores
    /// `.mieli`, legacy `.minne`, `*.files`, and non-markdown files.
    func testScanRecognizesStructureAndIgnoresInternal() throws {
        try makeFile(relative: "工作/项目A/技术方案.md")
        try makeFile(relative: "工作/周报.md")
        try makeFile(relative: "学习/Swift.md")
        try makeFile(relative: "学习/资料.pdf")          // non-md → skipped
        try makeDir("工作/项目A/技术方案.files") // attachment folder → skipped
        try makeFile(relative: "工作/项目A/技术方案.files/architecture.png")
        try makeDir(".mieli")                 // internal → skipped
        try makeFile(relative: ".mieli/index.sqlite")
        try makeDir(".muisti")                 // legacy internal → skipped
        try makeFile(relative: ".muisti/index.sqlite")
        try makeDir(".minne")                  // historical internal → skipped
        try makeFile(relative: ".minne/index.sqlite")
        try makeFile(relative: "README.md")

        let items = try WorkspaceScanner.scan(root)

        // Top level: 工作, 学习, README sorted with localizedStandardCompare
        // (Chinese sorts before ASCII; 工作 before 学习). Internal dirs omitted.
        XCTAssertEqual(items.map(\.name), ["工作", "学习", "README.md"])

        let work = try XCTUnwrap(items.first { $0.name == "工作" })
        XCTAssertEqual(work.kind, .folder)
        XCTAssertEqual(work.relativePath, "工作")
        XCTAssertEqual(work.children?.map(\.name), ["项目A", "周报.md"])

        let project = try XCTUnwrap(work.children?.first { $0.name == "项目A" })
        XCTAssertEqual(project.children?.map(\.name), ["技术方案.md"])
        XCTAssertEqual(project.children?.first?.relativePath, "工作/项目A/技术方案.md")
    }

    /// A flat workspace with only notes and no folders still scans.
    func testFlatWorkspace() throws {
        try makeFile(relative: "a.md")
        try makeFile(relative: "b.md")

        let items = try WorkspaceScanner.scan(root)
        XCTAssertEqual(items.map(\.name), ["a.md", "b.md"])
        XCTAssertTrue(items.allSatisfy { $0.kind == .note })
    }

    /// Scanning a non-existent root throws.
    func testScanNonexistentRootThrows() {
        let missing = root.appendingPathComponent("nope")
        XCTAssertThrowsError(try WorkspaceScanner.scan(missing))
    }
}
