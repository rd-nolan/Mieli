import XCTest
@testable import Mieli

/// Verifies T031: ULID generation used for the stable Note ID.
final class NoteIDTests: XCTestCase {

    private static let alphabet: [Character] =
        Array("0123456789ABCDEFGHJKMNPQRSTVWXYZ")

    /// Decodes the first 10 chars (timestamp) of a ULID back to milliseconds.
    private static func decodeTimestamp(_ ulid: String) -> UInt64 {
        var ms: UInt64 = 0
        for ch in ulid.prefix(10) {
            let value = alphabet.firstIndex(of: ch)!
            ms = (ms << 5) | UInt64(value)
        }
        return ms
    }

    func testGenerates26CharacterString() {
        let id = NoteID.generate()
        XCTAssertEqual(id.count, 26)
    }

    func testUsesOnlyCrockfordAlphabet() {
        let id = NoteID.generate()
        let allowed = Set(Self.alphabet)
        XCTAssertTrue(id.allSatisfy { allowed.contains($0) })
    }

    func testTimestampPrefixTracksNow() {
        let before = UInt64(Date().timeIntervalSince1970 * 1000)
        let id = NoteID.generate()
        let after = UInt64(Date().timeIntervalSince1970 * 1000)
        let decoded = Self.decodeTimestamp(id)
        XCTAssertGreaterThanOrEqual(decoded, before)
        XCTAssertLessThanOrEqual(decoded, after)
    }

    func testGeneratedIDsAreUnique() {
        var seen = Set<String>()
        for _ in 0..<10_000 {
            let id = NoteID.generate()
            XCTAssertFalse(seen.contains(id), "Duplicate ULID: \(id)")
            seen.insert(id)
        }
    }

    func testTimePrefixIsLexicographicallyOrdered() {
        // Simulate two IDs a millisecond apart via timestamps captured around
        // two Date snapshots. ULID should sort old < new.
        let id1 = NoteID.generate()
        Thread.sleep(forTimeInterval: 0.002)
        let id2 = NoteID.generate()
        XCTAssertLessThan(id1, id2)
    }

    func testIDsDifferFromSequentialCalls() {
        let a = NoteID.generate()
        let b = NoteID.generate()
        XCTAssertNotEqual(a, b)
    }
}