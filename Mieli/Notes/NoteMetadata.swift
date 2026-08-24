import Foundation

/// Builds the YAML Front Matter + body for a newly created note (AGENTS §9, §32).
///
/// A brand-new note is created with stable metadata: a fresh ULID `id`, an
/// empty `tags` list, and `created`/`updated` alike, both ISO-8601. The body
/// starts with a single H1 derived from the note title.
enum NoteMetadataFactory {

    private static let isoFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    /// Renders the full Markdown content for a new note.
    ///
    /// - Parameters:
    ///   - title: optional title. When empty or not supplied, the note body starts empty.
    ///   - id: reuse an existing stable id when supplied (e.g. rename); a fresh
    ///         ULID is generated when `nil`.
    ///   - now: timestamp stamped into `created`/`updated` (defaults to now).
    static func makeNoteContent(title: String = "", id: String? = nil, now: Date = Date()) -> String {
        let noteID = id ?? NoteID.generate()
        let iso = isoFormatter.string(from: now)
        let trimmedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let body = trimmedTitle.isEmpty ? "" : "\n\n# \(trimmedTitle)"
        return """
        ---
        id: \(noteID)
        tags: []
        created: \(iso)
        updated: \(iso)
        ---\(body)
        """
    }
}