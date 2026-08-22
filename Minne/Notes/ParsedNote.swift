import Foundation

/// A note parsed out of Markdown, ready to be indexed (AGENTS §23 / T045).
///
/// Built by reusing the existing Markdown-domain parsers (FrontMatter, NoteTitle,
/// PlainText). The filesystem metadata (mtime/size/hash) is added elsewhere by
/// the indexer — this keeps the Markdown-domain struct focused.
struct ParsedNote {
    let relativePath: String
    let filename: String
    let folder: String          // "" == workspace root
    let id: String
    let title: String
    let tags: [String]
    let createdAtISO: String?
    let updatedAtISO: String?
    let plainText: String

    /// Parses `markdown` for a note at `relativePath` (workspace-relative).
    ///
    /// A stable `id` is taken from the Front Matter when present; otherwise a
    /// fresh ULID is generated (a newly encountered note gets a stable id
    /// without rewriting the file).
    init(relativePath: String, markdown: String) {
        let ns = relativePath as NSString
        self.relativePath = relativePath
        self.filename = ns.lastPathComponent
        self.folder = ns.deletingLastPathComponent  // "" when at root

        let fm = FrontMatterParser.parse(markdown)
        self.id = fm?.id ?? NoteID.generate()
        self.title = NoteTitleParser.title(of: markdown, filename: self.filename)
        self.tags = fm?.tags ?? []
        self.createdAtISO = fm?.created.map(Self.isoString)
        self.updatedAtISO = fm?.updated.map(Self.isoString)
        self.plainText = PlainTextExtractor.plainText(from: markdown)
    }

    private static func isoString(_ date: Date) -> String {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f.string(from: date)
    }
}