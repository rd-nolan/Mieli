import XCTest
import AppKit
@testable import Mieli

final class EditorWebViewTests: XCTestCase {

    @MainActor
    func testStripNavigationItemsRemovesReloadAndNavigation() {
        let menu = NSMenu(title: "Context Menu")
        menu.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "")
        menu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "")
        menu.addItem(NSMenuItem.separator())
        menu.addItem(withTitle: "Reload", action: Selector(("reload:")), keyEquivalent: "")
        menu.addItem(withTitle: "重新载入", action: nil, keyEquivalent: "")
        menu.addItem(withTitle: "Back", action: Selector(("goBack:")), keyEquivalent: "")
        menu.addItem(withTitle: "Forward", action: Selector(("goForward:")), keyEquivalent: "")
        menu.addItem(NSMenuItem.separator())
        menu.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "")

        MarkdownEditorView.EditorWebView.stripNavigationItems(from: menu)

        let titles = menu.items.compactMap { $0.isSeparatorItem ? "---" : $0.title }
        XCTAssertFalse(titles.contains("Reload"))
        XCTAssertFalse(titles.contains("重新载入"))
        XCTAssertFalse(titles.contains("Back"))
        XCTAssertFalse(titles.contains("Forward"))
        XCTAssertTrue(titles.contains("Cut"))
        XCTAssertTrue(titles.contains("Copy"))
        XCTAssertTrue(titles.contains("Paste"))

        // Ensure no redundant adjacent or trailing separators
        XCTAssertEqual(titles, ["Cut", "Copy", "---", "Paste"])
    }

    @MainActor
    func testStripNavigationItemsRemovesNestedSubmenuReload() {
        let menu = NSMenu(title: "Context Menu")
        let subMenu = NSMenu(title: "Sub")
        subMenu.addItem(withTitle: "Reload", action: Selector(("reload:")), keyEquivalent: "")
        subMenu.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "")
        let subItem = NSMenuItem(title: "Sub", action: nil, keyEquivalent: "")
        subItem.submenu = subMenu
        menu.addItem(subItem)

        MarkdownEditorView.EditorWebView.stripNavigationItems(from: menu)

        let subTitles = subMenu.items.map(\.title)
        XCTAssertFalse(subTitles.contains("Reload"))
        XCTAssertTrue(subTitles.contains("Copy"))
    }
}
