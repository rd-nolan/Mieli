import XCTest
@testable import Minne

/// Verifies T034: reading tags from a note's Front Matter.
final class NoteTagsTests: XCTestCase {

    func testReadsBlockTags() {
        let md = """
        ---
        id: 1
        tags:
          - Swift
          - macOS
        ---
        # 正文
        """
        XCTAssertEqual(NoteTags.tags(in: md), ["Swift", "macOS"])
    }

    func testReadsInlineTags() {
        let md = "---\ntags: [a, b]\n---\n"
        XCTAssertEqual(NoteTags.tags(in: md), ["a", "b"])
    }

    func testReadsChineseTags() {
        let md = """
        ---
        tags:
          - 状态机
          - Swift 并发
        ---
        """
        XCTAssertEqual(NoteTags.tags(in: md), ["状态机", "Swift 并发"])
    }

    func testReturnsEmptyWhenNoFrontMatter() {
        XCTAssertEqual(NoteTags.tags(in: "# 没有 FM\n"), [])
    }

    func testReturnsEmptyWhenTagsKeyMissing() {
        let md = """
        ---
        id: 7
        ---
        """
        XCTAssertEqual(NoteTags.tags(in: md), [])
    }

    func testReturnsEmptyWhenTagsEmpty() {
        let md = "---\ntags: []\n---\n"
        XCTAssertEqual(NoteTags.tags(in: md), [])
    }

    func testStateTagDoesNotLeakIntoOtherLines() {
        // A banned: FM 中 `tags:` 只影响其后列表行，body 中 `- 项` 不算 tag。
        let md = """
        ---
        tags:
          - 真标签
        ---
        - 列表项
        """
        XCTAssertEqual(NoteTags.tags(in: md), ["真标签"])
    }

    // MARK: - T071 addTag

    func testAddTagAppendsToExistingList() {
        let md = """
        ---
        id: 1
        tags:
          - Swift
        ---
        # 正文
        """
        let out = NoteTags.addTag("macOS", to: md)
        XCTAssertEqual(NoteTags.tags(in: out), ["Swift", "macOS"])
        // Body and unrelated id key are preserved.
        XCTAssertTrue(out.contains("id: 1"))
        XCTAssertTrue(out.contains("# 正文"))
        XCTAssertTrue(out.contains("  - macOS\n"))
    }

    func testAddTagInsertsListWhenMissing() {
        let md = "---\nid: 2\n---\n正文\n"
        let out = NoteTags.addTag("标签A", to: md)
        XCTAssertEqual(NoteTags.tags(in: out), ["标签A"])
        XCTAssertTrue(out.contains("id: 2"))
        XCTAssertTrue(out.contains("正文"))
    }

    func testAddTagConvertsEmptyInlineArrayUsedByNewNotes() {
        let md = "---\nid: 3\ntags: []\n---\n# 新笔记\n"
        let out = NoteTags.addTag("T115", to: md)

        XCTAssertEqual(NoteTags.tags(in: out), ["T115"])
        XCTAssertFalse(out.contains("tags: []"))
        XCTAssertTrue(out.contains("tags:\n  - T115\n"))
        XCTAssertTrue(out.contains("# 新笔记"))
    }

    func testAddTagConvertsPopulatedInlineArrayWithoutLosingTags() {
        let md = "---\ntags: [Swift, macOS]\n---\n"
        let out = NoteTags.addTag("Editor", to: md)

        XCTAssertEqual(NoteTags.tags(in: out), ["Swift", "macOS", "Editor"])
        XCTAssertFalse(out.contains("tags: ["))
    }

    func testAddTagPrependsBlockWhenNoFrontMatter() {
        let md = "# 无 FM\n"
        let out = NoteTags.addTag("新建", to: md)
        XCTAssertEqual(NoteTags.tags(in: out), ["新建"])
        XCTAssertTrue(out.contains("# 无 FM"))
    }

    func testAddTagIsIdempotent() {
        let md = "---\ntags:\n  - Swift\n---\n"
        XCTAssertEqual(NoteTags.addTag("Swift", to: md), md)
        XCTAssertEqual(NoteTags.addTag("   ", to: md), md)
    }

    func testAddTagChineseAndSpacedValues() {
        let md = "---\ntags:\n  - 状态机\n---\n"
        let out = NoteTags.addTag("Swift 并发", to: md)
        XCTAssertEqual(NoteTags.tags(in: out), ["状态机", "Swift 并发"])
    }

    // MARK: - T072 removeTag

    func testRemoveTagKeepsOthersAndBody() {
        let md = """
        ---
        id: 9
        tags:
          - Swift
          - macOS
        ---
        # 正文
        - Body 中的项
        """
        let out = NoteTags.removeTag("Swift", from: md)
        XCTAssertEqual(NoteTags.tags(in: out), ["macOS"])
        XCTAssertTrue(out.contains("# 正文"))
        // Body list items must not be touched.
        XCTAssertTrue(out.contains("- Body 中的项"))
        XCTAssertTrue(out.contains("id: 9"))
    }

    func testRemoveTagRemovesEmptyTagsKey() {
        let md = "---\ntags:\n  - 唯一\n---\n"
        let out = NoteTags.removeTag("唯一", from: md)
        XCTAssertEqual(NoteTags.tags(in: out), [])
        // The now-empty `tags:` key should be gone, not left empty.
        let hasTagsKey = out.contains("tags:")
        XCTAssertFalse(hasTagsKey)
    }

    func testRemoveTagIsIdempotent() {
        let md = "---\ntags:\n  - Swift\n---\n"
        XCTAssertEqual(NoteTags.removeTag("macOS", from: md), md) // not present
        XCTAssertEqual(NoteTags.removeTag("   ", from: md), md)    // blank
    }

    func testRemoveTagChinese() {
        let md = "---\ntags:\n  - 状态机\n  - 并发并发\n---\n"
        let out = NoteTags.removeTag("状态机", from: md)
        XCTAssertEqual(NoteTags.tags(in: out), ["并发并发"])
    }
}
