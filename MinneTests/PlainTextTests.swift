import XCTest
@testable import Minne

/// Verifies T035: Markdown → plain text suitable for full-text search.
final class PlainTextTests: XCTestCase {

    func testStripsFrontMatter() {
        let md = """
        ---
        id: 01X
        tags: [私有]
        updated: 2026-01-01
        ---
        # 正文标题
        """
        let out = PlainTextExtractor.plainText(from: md)
        XCTAssertFalse(out.contains("01X"))
        XCTAssertFalse(out.contains("私有"))
        XCTAssertTrue(out.contains("正文标题"))
    }

    func testHeadingLosesMarker() {
        XCTAssertEqual(
            PlainTextExtractor.plainText(from: "# 标题").trimmingCharacters(in: .whitespacesAndNewlines),
            "标题")
    }

    func testEmphasisAndStrongRemoved() {
        let md = "**粗体** 和 *斜体* 和 _下划线_"
        let out = PlainTextExtractor.plainText(from: md).trimmingCharacters(in: .whitespaces)
        XCTAssertTrue(out.contains("粗体"))
        XCTAssertTrue(out.contains("斜体"))
        XCTAssertTrue(out.contains("下划线"))
        XCTAssertFalse(out.contains("*"))
        XCTAssertFalse(out.contains("_"))
    }

    func testInlineCodeStripped() {
        let md = "命令是 `git commit`"
        let out = PlainTextExtractor.plainText(from: md).trimmingCharacters(in: .whitespacesAndNewlines)
        XCTAssertEqual(out, "命令是 git commit")
    }

    func testLinkBecomesText() {
        let md = "查看 [文档](https://example.com)"
        let out = PlainTextExtractor.plainText(from: md).trimmingCharacters(in: .whitespaces)
        XCTAssertTrue(out.contains("查看 文档"))
        XCTAssertFalse(out.contains("https://example.com"))
    }

    func testImageBecomesAlt() {
        let md = "![架构图](./架构.files/arch.png)"
        let out = PlainTextExtractor.plainText(from: md).trimmingCharacters(in: .whitespacesAndNewlines)
        XCTAssertEqual(out, "架构图")
    }

    func testChinesePreserved() {
        let md = "# Swift 状态机\n今天研究了 Spring 状态机的实现方案"
        let out = PlainTextExtractor.plainText(from: md)
        XCTAssertTrue(out.contains("Swift 状态机"))
        XCTAssertTrue(out.contains("今天研究了 Spring 状态机的实现方案"))
    }

    func testCodeFenceContentKeptWithoutBackticks() {
        let md = "```swift\nlet x = 1\n```\n正文"
        let out = PlainTextExtractor.plainText(from: md)
        XCTAssertTrue(out.contains("let x = 1"))
        XCTAssertFalse(out.contains("```"))
    }

    func testBulletAndOrderedStrip() {
        let md = "- 甲\n1. 乙\n   - 丙"
        let out = PlainTextExtractor.plainText(from: md)
        XCTAssertTrue(out.contains("甲"))
        XCTAssertTrue(out.contains("乙"))
        XCTAssertTrue(out.contains("丙"))
    }

    func testCollapsesBlankLines() {
        let md = "# A\n\n\n\nB\n\nC"
        let out = PlainTextExtractor.plainText(from: md)
        XCTAssertTrue(out.contains("A\nB"))
        XCTAssertFalse(out.contains("\n\n\n"))
    }

    func testNoMarkdownReturnsTrimmedInput() {
        let md = "  纯文本  内容  "
        let out = PlainTextExtractor.plainText(from: md)
        XCTAssertEqual(out.trimmingCharacters(in: .whitespacesAndNewlines), "纯文本  内容")
    }
}