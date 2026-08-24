import Foundation

/// ULID generator (Crockford Base32, 26 chars) used for Mieli's stable Note ID.
///
/// Layout: 10 chars (48-bit millisecond timestamp) + 16 chars (80-bit random).
/// Standard alphabet: `0123456789ABCDEFGHJKMNPQRSTVWXYZ` (no I, L, O, U).
///
/// The ID lives in the note's YAML Front Matter (`id:`). Renaming or moving a
/// note only touches the filesystem (never rewrites the file body), so the ID
/// is stable across rename/move by construction.
enum NoteID {
    static let alphabet: [Character] =
        Array("0123456789ABCDEFGHJKMNPQRSTVWXYZ")

    /// Generates a new, lexically time-ordered ULID.
    static func generate() -> String {
        var chars = [Character](repeating: "0", count: 26)

        // 1) Timestamp prefix: current milliseconds since epoch -> 10 chars.
        let ms = UInt64(Date().timeIntervalSince1970 * 1000)
        var time = ms
        // 48 bits / 5 bits per char = 10 chars (last char uses 3 bits).
        for i in stride(from: 9, through: 0, by: -1) {
            chars[i] = alphabet[Int(time & 0x1F)]
            time >>= 5
        }

        // 2) Random suffix: 16 chars, each an independent 5-bit value.
        for i in 10..<26 {
            chars[i] = alphabet[Int.random(in: 0..<32)]
        }

        return String(chars)
    }
}