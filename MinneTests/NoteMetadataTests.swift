import XCTest
@testable import Minne

/// Verifies T032: new notes are created with full YAML Front Matter.
final class NoteMetadataTests: XCTestCase {

    func testGeneratedContentHasAllFourFields() throws {
        let content = NoteMetadataFactory.makeNoteContent(title: "周报")
        let fm = try XCTUnwrap(FrontMatterParser.parse(content))
        XCTAssertEqual(fm.id?.count, 26)
        XCTAssertEqual(fm.tags, [])
        XCTAssertNotNil(fm.created)
        XCTAssertNotNil(fm.updated)
    }

    func testCreatedEqualsUpdatedForNewNote() throws {
        let content = NoteMetadataFactory.makeNoteContent(title: "x", now: Date(timeIntervalSince1970: 1_750_000_000))
        let fm = try XCTUnwrap(FrontMatterParser.parse(content))
        XCTAssertEqual(fm.created, fm.updated)
        XCTAssertEqual(fm.created, Date(timeIntervalSince1970: 1_750_000_000))
    }

    func testReusesSuppliedID() throws {
        let content = NoteMetadataFactory.makeNoteContent(title: "t", id: "01K32M4PZXXXXXXXX")
        let fm = try XCTUnwrap(FrontMatterParser.parse(content))
        XCTAssertEqual(fm.id, "01K32M4PZXXXXXXXX")
    }

    func testUsesTitleAsH1() {
        let content = NoteMetadataFactory.makeNoteContent(title: "Swift 并发")
        XCTAssertTrue(content.hasSuffix("# Swift 并发"))
    }

    func testStableIDActualRoundTripThroughCreateNote() throws {
        // Two notes must get distinct stable ids.
        let a = NoteMetadataFactory.makeNoteContent(title: "a")
        let b = NoteMetadataFactory.makeNoteContent(title: "b")
        let fmA = try XCTUnwrap(FrontMatterParser.parse(a))
        let fmB = try XCTUnwrap(FrontMatterParser.parse(b))
        XCTAssertNotEqual(fmA.id, fmB.id)
    }
}