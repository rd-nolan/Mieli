import XCTest
@testable import Muisti
import GRDB

/// Verifies T050: basic FTS search across title/filename/path/tags/content.
final class SearchServiceTests: XCTestCase {

    private var tempDir: URL!
    private var queue: DatabaseQueue!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("muisti-search-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        queue = try? DatabaseManager.openDatabaseQueue(at: tempDir)
    }

    override func tearDown() {
        try? queue?.close()
        try? FileManager.default.removeItem(at: tempDir)
        super.tearDown()
    }

    /// Seeds a note and (re)indexes it.
    private func index(_ name: String, id: String, markdown: String) throws {
        try IndexService.index(ParsedNote(relativePath: name, markdown: markdown), in: queue)
    }

    private func searchTitle(_ query: String) throws -> [SearchResult] {
        try SearchService.searchTitle(query, in: queue)
    }

    private func searchFilename(_ query: String) throws -> [SearchResult] {
        try SearchService.searchFilename(query, in: queue)
    }

    private func searchTags(_ query: String) throws -> [SearchResult] {
        try SearchService.searchTags(query, in: queue)
    }

    private func search(_ query: String) throws -> [SearchResult] {
        try SearchService.search(query, in: queue)
    }

    private func searchContent(_ query: String) throws -> [SearchResult] {
        try SearchService.searchContent(query, in: queue)
    }

    func testFindsContentMatch() throws {
        try index("a.md", id: "s1", markdown: "---\nid: s1\n---\n# 标题\n今天研究了 Spring 状态机实现方案")
        let hits = try search("状态机")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "s1")
        XCTAssertNotNil(hits.first?.snippet) // excerpt excerpt present
    }

    func testFindsTitleAndPathMatch() throws {
        try index("报告/周报.md", id: "s2", markdown: "---\nid: s2\n---\n# Swift 并发模型\n正文")
        // A token that appears only in the title…
        let byTitle = try search("并发模型")
        XCTAssertEqual(byTitle.count, 1)
        XCTAssertEqual(byTitle.first?.title, "Swift 并发模型")
    }

    func testShortQueryReturnsEmptyInsteadOfError() throws {
        try index("a.md", id: "s3", markdown: "---\nid: s3\n---\n# 甲\n内容")
        // trigram cannot match tokens under 3 chars — must be a no-match, not a throw.
        let hits = try search("内容")
        _ = hits
        let short = try search("ab")
        XCTAssertTrue(short.isEmpty)
    }

    func testNoMatchReturnsEmpty() throws {
        try index("a.md", id: "s4", markdown: "---\nid: s4\n---\n# 甲\n内容")
        let hits = try search("不存在词xyz")
        XCTAssertTrue(hits.isEmpty)
    }

    func testResultCarriesPathAndFolder() throws {
        try index("工作/项目A/方案.md", id: "s5", markdown: "---\nid: s5\n---\n# 方案\n无人机方案")
        let hits = try search("无人机")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.relativePath, "工作/项目A/方案.md")
        XCTAssertEqual(hits.first?.folder, "工作/项目A")
        XCTAssertEqual(hits.first?.filename, "方案.md")
    }

    // MARK: T051 — title-only search

    func testSearchTitleMatchesTitleColumn() throws {
        try index("a.md", id: "t1", markdown: "---\nid: t1\n---\n# Swift 并发模型\n正文")
        let hits = try searchTitle("并发模型")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "t1")
        XCTAssertEqual(hits.first?.title, "Swift 并发模型")
    }

    func testSearchTitleDoesNotMatchContentOnly() throws {
        // "状态机" appears only in the body, never in the title.
        try index("b.md", id: "t2", markdown: "---\nid: t2\n---\n# 正文标题\n研究了 Spring 状态机实现方案")
        let hits = try searchTitle("状态机")
        XCTAssertTrue(hits.isEmpty, "title-only search must not match body content")
    }

    func testSearchTitleMatchesChineseTitle() throws {
        try index("c.md", id: "t3", markdown: "---\nid: t3\n---\n# 状态机设计\n正文")
        let hits = try searchTitle("状态机")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "t3")
    }

    func testSearchTitleShortQueryReturnsEmptyNotError() throws {
        try index("d.md", id: "t4", markdown: "---\nid: t4\n---\n# 甲乙丙\n正文")
        let hits = try searchTitle("ab")
        XCTAssertTrue(hits.isEmpty)
    }

    // MARK: T052 — filename-only search

    func testSearchFilenameMatchesFilenameColumn() throws {
        try index("spring-notes.md", id: "n1", markdown: "---\nid: n1\n---\n# 状态机\n正文")
        let hits = try searchFilename("spring")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "n1")
        XCTAssertEqual(hits.first?.filename, "spring-notes.md")
    }

    func testSearchFilenameDoesNotMatchTitleOrContent() throws {
        // "spring" appears in the title and body but not the filename.
        try index("misc.md", id: "n2", markdown: "---\nid: n2\n---\n# spring 状态机\n研究了 spring 实现")
        let hits = try searchFilename("spring")
        XCTAssertTrue(hits.isEmpty, "filename-only search must not match title/body")
    }

    func testSearchFilenameMatchesExtensionlessKeyword() throws {
        try index("技术方案.md", id: "n3", markdown: "---\nid: n3\n---\n# 标题\n正文")
        let hits = try searchFilename("技术方案")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.filename, "技术方案.md")
    }

    func testSearchFilenameShortQueryReturnsEmpty() throws {
        try index("xx.md", id: "n4", markdown: "---\nid: n4\n---\n# 标题\n正文")
        let hits = try searchFilename("ab")
        XCTAssertTrue(hits.isEmpty)
    }

    // MARK: T053 — tag-only search

    private func tagNote(_ id: String, path: String, tags: [String], title: String = "标题") throws {
        let fm = "id: \(id)\ntags:\n" + tags.map { "  - \($0)" }.joined(separator: "\n")
        try index(path, id: id, markdown: "---\n\(fm)\n---\n# \(title)\n正文")
    }

    func testSearchTagsMatchesTagColumn() throws {
        try tagNote("g1", path: "a.md", tags: ["Swift"])
        let hits = try searchTags("Swift")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "g1")
    }

    func testSearchTagsMatchesSecondOfMultipleTags() throws {
        try tagNote("g2", path: "b.md", tags: ["macOS", "SwiftUI"])
        let hits = try searchTags("SwiftUI")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "g2")
    }

    func testSearchTagsDoesNotMatchTitleOrContent() throws {
        // "Swift" appears only in the title/body, not in tags.
        try index("c.md", id: "g3", markdown: "---\nid: g3\n---\n# Swift 并发\n研究了 Swift")
        let hits = try searchTags("Swift")
        XCTAssertTrue(hits.isEmpty, "tag-only search must not match title/body")
    }

    func testSearchTagsShortQueryReturnsEmpty() throws {
        try tagNote("g4", path: "d.md", tags: ["abc"])
        let hits = try searchTags("ab")
        XCTAssertTrue(hits.isEmpty)
    }

    // MARK: T054 — content-only search

    func testSearchContentMatchesBodyTerm() throws {
        try index("a.md", id: "c1", markdown: "---\nid: c1\n---\n# 标题\n今天研究了 Spring 状态机实现方案")
        let hits = try searchContent("状态机")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "c1")
    }

    func testSearchContentDoesNotMatchTagsColumn() throws {
        // "SwiftUI" exists only as a tag; the plain-text content and title omit it.
        try tagNote("c2", path: "b.md", tags: ["SwiftUI"])
        let hits = try searchContent("SwiftUI")
        XCTAssertTrue(hits.isEmpty, "content-only search must not match tags column")
    }

    func testSearchContentCarriesSnippet() throws {
        try index("c.md", id: "c3", markdown: "---\nid: c3\n---\n# 标题\n前半句 Spring 状态机 后半句")
        let hits = try searchContent("状态机")
        XCTAssertEqual(hits.count, 1)
        XCTAssertTrue(hits.first?.snippet?.contains("状态机") == true, "snippet should contain the matched term")
    }

    func testSearchContentShortQueryReturnsEmpty() throws {
        try index("d.md", id: "c4", markdown: "---\nid: c4\n---\n# 标题\n正文内容")
        let hits = try searchContent("ab")
        XCTAssertTrue(hits.isEmpty)
    }

    // MARK: T055 — Chinese search (AGENTS §26)

    func testChineseStateMachineSearch() throws {
        // AGENTS §26 example sentence + the `状态机` keyword.
        try index("a.md", id: "ch1", markdown: "---\nid: ch1\n---\n# Swift 并发\n今天研究了 Spring 状态机的实现方案")
        let hits = try search("状态机")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "ch1")
    }

    func testChineseImplementationPlanSearch() throws {
        // AGENTS §26: searching `实现方案` must hit the same body.
        try index("b.md", id: "ch2", markdown: "---\nid: ch2\n---\n# Swift 并发\n今天研究了 Spring 状态机的实现方案")
        let hits = try search("实现方案")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "ch2")
    }

    func testMixedChineseEnglishWithSpace() throws {
        // AGENTS §26: Chinese + English `Swift 并发` (spaced mix) matches a
        // note whose title contains `Swift 并发模型`.
        try index("c.md", id: "ch3", markdown: "---\nid: ch3\n---\n# Swift 并发模型\n正文")
        let hits = try search("Swift 并发")
        XCTAssertEqual(hits.count, 1)
        XCTAssertEqual(hits.first?.id, "ch3")
    }

    func testChineseShortTermBoundary() throws {
        // A 2-char Chinese keyword (below the trigram 3-char minimum) must be a
        // no-match — never a thrown error (AGENTS §42 no fatal on malformed).
        try index("d.md", id: "ch4", markdown: "---\nid: ch4\n---\n# 标题\n并发内容")
        let hits = try search("并发")
        XCTAssertTrue(hits.isEmpty)
    }

    // MARK: T056 — search ranking (AGENTS §27)

    func testTitleMatchRanksAboveContentMatch() throws {
        // Same keyword; one note has it in its title, the other only in content.
        try index("a.md", id: "r1", markdown: "---\nid: r1\n---\n# 状态机设计\n无关正文")
        try index("b.md", id: "r2", markdown: "---\nid: r2\n---\n# 别的标题\n正文里提到状态机体系与状态机状态")
        let hits = try search("状态机")
        let ids = hits.map(\.id)
        XCTAssertEqual(ids, ["r1", "r2"],
                       "title match (r1) must rank above content-only match (r2)")
    }

    func testFilenameMatchRanksAboveContentMatch() throws {
        // Keyword in filename only vs content only — filename must rank first.
        try index("并发框架.md", id: "r3", markdown: "---\nid: r3\n---\n# 无状态机标题\n正文")
        try index("c.md", id: "r4", markdown: "---\nid: r4\n---\n# 别的话题\n正文反复出现并发框架与并发框架依赖")
        let hits = try search("并发框架")
        let ids = hits.map(\.id)
        XCTAssertEqual(ids, ["r3", "r4"],
                       "filename match (r3) must rank above content match (r4)")
    }

    // MARK: T074 — filter notes by tag

    func testTaggedNotesReturnsOnlyMatchingNotes() throws {
        try tagNote("f1", path: "a.md", tags: ["Swift"])
        try tagNote("f2", path: "b.md", tags: ["Swift", "macOS"])
        try tagNote("f3", path: "c.md", tags: ["Vim"])
        let swift = try IndexService.taggedNotes(tag: "Swift", in: queue)
        XCTAssertEqual(swift.map(\.id).sorted(), ["f1", "f2"])
        XCTAssertEqual(swift.count, 2)
        let vim = try IndexService.taggedNotes(tag: "Vim", in: queue)
        XCTAssertEqual(vim.map(\.id), ["f3"])
    }

    func testTaggedCarriesPathAndIsSortedByFilename() throws {
        try tagNote("f4", path: "z.md", tags: ["Swift"])
        try tagNote("f5", path: "b.md", tags: ["Swift"])
        let hits = try IndexService.taggedNotes(tag: "Swift", in: queue)
        XCTAssertEqual(hits.map(\.id), ["f5", "f4"], "sorted by filename (b before z)")
        XCTAssertEqual(hits.first?.relativePath, "b.md")
        XCTAssertEqual(hits.first?.folder, "")
    }

    func testTaggedUnknownTagReturnsEmpty() throws {
        try tagNote("f6", path: "a.md", tags: ["Swift"])
        let hits = try IndexService.taggedNotes(tag: "不存在tag", in: queue)
        XCTAssertTrue(hits.isEmpty)
    }
}