import XCTest
import Testing
import Foundation
@testable import Muisti

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

@Suite("T126 tag name resolution")
struct TagNameResolutionTests {
    @Test("blank input cancels")
    func blankInput() {
        #expect(NoteTags.resolveTag("  \n", currentTags: [], workspaceTags: []) == nil)
    }

    @Test("new input is trimmed")
    func trimsNewTag() {
        #expect(NoteTags.resolveTag("  Swift 并发  ", currentTags: [], workspaceTags: []) == "Swift 并发")
    }

    @Test("current note match wins ignoring case")
    func currentNoteWins() {
        #expect(NoteTags.resolveTag(
            "swift",
            currentTags: ["SWIFT"],
            workspaceTags: ["swift", "Swift"]
        ) == "SWIFT")
    }

    @Test("exact workspace spelling wins among historical variants")
    func exactWorkspaceMatchWins() {
        #expect(NoteTags.resolveTag(
            "swift",
            currentTags: [],
            workspaceTags: ["Swift", "swift"]
        ) == "swift")
    }

    @Test("case-insensitive workspace matches have deterministic spelling")
    func workspaceFallbackIsStable() {
        #expect(NoteTags.resolveTag(
            "sWiFt",
            currentTags: [],
            workspaceTags: ["swift", "Swift"]
        ) == "Swift")
    }
}

@Suite("T128 runtime language")
struct AppLanguageTests {
    private func defaults() -> (UserDefaults, String) {
        let suite = "MuistiTests.AppLanguage.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        return (defaults, suite)
    }

    @Test("system language resolves Chinese and English")
    func systemResolution() {
        let (defaults, suite) = defaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let chinese = AppLanguage(
            defaults: defaults,
            preferenceKey: "language",
            preferredLanguages: ["zh-Hans-CN"]
        )
        #expect(chinese.resolvedIdentifier == "zh-Hans")

        let english = AppLanguage(
            defaults: defaults,
            preferenceKey: "other-language",
            preferredLanguages: ["fr-FR"]
        )
        #expect(english.resolvedIdentifier == "en")
    }

    @Test("explicit selection persists")
    func persistence() {
        let (defaults, suite) = defaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let first = AppLanguage(
            defaults: defaults,
            preferenceKey: "language",
            preferredLanguages: ["en-US"]
        )
        first.selection = .simplifiedChinese

        let restored = AppLanguage(
            defaults: defaults,
            preferenceKey: "language",
            preferredLanguages: ["en-US"]
        )
        #expect(restored.selection == .simplifiedChinese)
        #expect(restored.resolvedIdentifier == "zh-Hans")
    }

    @Test("legacy language preference migrates to Muisti")
    func legacyPreferenceMigrates() {
        let (defaults, suite) = defaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        defaults.set("zh-Hans", forKey: "Minne.AppLanguage")
        let language = AppLanguage(
            defaults: defaults,
            legacyDefaults: defaults,
            preferredLanguages: ["en-US"]
        )

        #expect(language.selection == .simplifiedChinese)
        #expect(defaults.string(forKey: AppLanguage.preferenceKey) == "zh-Hans")
    }

    @Test("language menu has three mutually exclusive choices")
    func menuState() {
        let (defaults, suite) = defaults()
        defer { defaults.removePersistentDomain(forName: suite) }
        let language = AppLanguage(
            defaults: defaults,
            preferenceKey: "language",
            preferredLanguages: ["en-US"]
        )

        #expect(AppLanguage.Selection.allCases.count == 3)
        for selection in AppLanguage.Selection.allCases {
            language.selection = selection
            #expect(AppLanguage.Selection.allCases.filter(language.isSelected).count == 1)
            #expect(language.isSelected(selection))
        }
    }

    @Test("critical catalog values exist in both languages")
    func localizedValues() {
        let (defaults, suite) = defaults()
        defer { defaults.removePersistentDomain(forName: suite) }
        let language = AppLanguage(
            defaults: defaults,
            preferenceKey: "language",
            preferredLanguages: ["en-US"]
        )

        language.selection = .english
        #expect(language.text("New Note") == "New Note")
        #expect(language.text("Operation Failed") == "Operation Failed")

        language.selection = .simplifiedChinese
        #expect(language.text("New Note") == "新建笔记")
        #expect(language.text("Operation Failed") == "操作失败")
    }
}
