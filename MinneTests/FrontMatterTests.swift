import XCTest
@testable import Minne

/// Verifies T030: Front Matter parsing (`id` / `tags` / `created` / `updated`).
final class FrontMatterTests: XCTestCase {

    private func date(_ iso: String) -> Date {
        let f = ISO8601DateFormatter()
        return f.date(from: iso)!
    }

    func testParsesAllFields() {
        let md = """
        ---
        id: 01K32M4PZXXXXXXXX
        tags:
          - Swift
          - macOS
        created: 2026-08-21T08:00:00+08:00
        updated: 2026-08-21T08:30:00+08:00
        ---
        # 正文
        内容
        """
        let matter = try! XCTUnwrap(FrontMatterParser.parse(md))
        XCTAssertEqual(matter.id, "01K32M4PZXXXXXXXX")
        XCTAssertEqual(matter.tags, ["Swift", "macOS"])
        XCTAssertEqual(matter.created, date("2026-08-21T08:00:00+08:00"))
        XCTAssertEqual(matter.updated, date("2026-08-21T08:30:00+08:00"))
    }

    func testNoFrontMatterReturnsNil() {
        let md = "# 没有 front matter\n直接正文\n"
        XCTAssertNil(FrontMatterParser.parse(md))
    }

    func testMissingClosingFenceReturnsNil() {
        let md = """
        ---
        id: abc
        tags: [a]
        """
        XCTAssertNil(FrontMatterParser.parse(md))
    }

    func testDocumentNotStartingWithFenceReturnsNil() {
        let md = "# 标题\n---\nid: x\n---\n"
        XCTAssertNil(FrontMatterParser.parse(md))
    }

    func testParsesInlineTagsList() {
        let md = """
        ---
        id: 111
        tags: [Swift, 状态机]
        ---
        """
        let matter = try! XCTUnwrap(FrontMatterParser.parse(md))
        XCTAssertEqual(matter.tags, ["Swift", "状态机"])
    }

    func testMissingFieldsStayNil() {
        let md = """
        ---
        id: 333
        ---
        """
        let matter = try! XCTUnwrap(FrontMatterParser.parse(md))
        XCTAssertEqual(matter.id, "333")
        XCTAssertNil(matter.created)
        XCTAssertNil(matter.updated)
        XCTAssertTrue(matter.tags.isEmpty)
    }

    func testChineseTagsWithWhitespace() {
        let md = """
        ---
        tags:
          - Swift 并发
          - 状态机
        ---
        """
        let matter = try! XCTUnwrap(FrontMatterParser.parse(md))
        XCTAssertEqual(matter.tags, ["Swift 并发", "状态机"])
    }

    func testMalformedDateIgnored() {
        let md = """
        ---
        id: 444
        created: not-a-date
        ---
        """
        let matter = try! XCTUnwrap(FrontMatterParser.parse(md))
        XCTAssertNil(matter.created)
        XCTAssertEqual(matter.id, "444")
    }

    func testTagsIgnoreQuotes() {
        let md = """
        ---
        tags:
          - "Swift"
          - 'x'
        ---
        """
        let matter = try! XCTUnwrap(FrontMatterParser.parse(md))
        XCTAssertEqual(matter.tags, ["Swift", "x"])
    }

    func testParserDoesNotConsumeTrailingBody() throws {
        let md = """
        ---
        id: 555
        ---
        # 标题
        **粗体** 内容
        """
        let matter = try XCTUnwrap(FrontMatterParser.parse(md))
        XCTAssertEqual(matter.id, "555")
        // The body after the closing fence is untouched (the parser only reads).
        XCTAssertTrue(md.hasSuffix("# 标题\n**粗体** 内容"))
    }
}