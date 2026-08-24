import XCTest
@testable import Mieli

/// Verifies T033: note title resolves from first H1, falling back to filename.
final class NoteTitleTests: XCTestCase {

    func testUsesFirstH1() {
        let md = "# Swift 并发\n正文\n# 第二个标题\n"
        XCTAssertEqual(NoteTitleParser.title(of: md, filename: "note.md"), "Swift 并发")
    }

    func testPicksFirstH1WhenMultipleH1s() {
        let md = "# 甲\n## 乙\n# 丙\n"
        XCTAssertEqual(NoteTitleParser.title(of: md, filename: "x.md"), "甲")
    }

    func testSkipsH2AndUsesFirstH1() {
        let md = "## 小标题\n# 正式标题\n"
        XCTAssertEqual(NoteTitleParser.title(of: md, filename: "x.md"), "正式标题")
    }

    func testFallsBackToFilenameWhenNoH1() {
        let md = "**粗体** 内容，无标题\n"
        XCTAssertEqual(NoteTitleParser.title(of: md, filename: "hello.md"), "hello")
    }

    func testFilenameFallbackDropsPathComponent() {
        let md = "普通段落\n"
        XCTAssertEqual(NoteTitleParser.title(of: md, filename: "归档/技术方案.md"), "技术方案")
    }

    func testEmptyContentUsesFilename() {
        XCTAssertEqual(NoteTitleParser.title(of: "", filename: "空.md"), "空")
    }

    func testIgnoresHeadingWithoutSpace() {
        let md = "#无空格不算标题\n"
        XCTAssertEqual(NoteTitleParser.title(of: md, filename: "a.md"), "a")
    }

    func testH1StripsTrailingClosingHashes() {
        let md = "# 标题 ##\n"
        XCTAssertEqual(NoteTitleParser.title(of: md, filename: "b.md"), "标题")
    }

    func testH1TrimsWhitespace() {
        let md = "#  两周空格  \n"
        XCTAssertEqual(NoteTitleParser.title(of: md, filename: "c.md"), "两周空格")
    }
}