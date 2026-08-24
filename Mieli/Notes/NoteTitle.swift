import Foundation

/// Resolves a note's display title (AGENTS §11).
///
/// The first H1 (`# Heading`) in the Markdown body wins; when the document has
/// no H1, the filename (sans extension) is used as a fallback.
enum NoteTitleParser {

    /// - Parameters:
    ///   - content: the full Markdown body of the note.
    ///   - filename: the note's filename *with* extension (e.g. `hello.md`);
    ///         used (base name, no extension) only when no H1 exists.
    static func title(of content: String, filename: String) -> String {
        for line in content.components(separatedBy: .newlines) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            // Strict H1: a single `#` followed by a space. `## ` is H2 and
            // is deliberately skipped; `#no-space` is not a heading.
            guard trimmed.hasPrefix("# "), trimmed != "#" else { continue }

            var heading = trimmed.dropFirst(2).trimmingCharacters(in: .whitespaces)
            // Tolerate a trailing closing sequence, e.g. `# Title ##` → `Title`.
            heading = heading.replacingOccurrences(
                of: #"\s+#+\s*$"#, with: "", options: .regularExpression)
            if !heading.isEmpty { return heading }
        }

        // Fallback: filename without its extension.
        let base = (filename as NSString).deletingPathExtension
        return (base as NSString).lastPathComponent
    }
}