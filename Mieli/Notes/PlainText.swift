import Foundation

/// Extracts plain searchable text from Markdown (AGENTS § 23, T035).
///
/// This is deliberately NOT a Markdown renderer. It strips syntax tokens and
/// the YAML Front Matter so the result is suitable for full-text search:
/// headings loses their `#`, emphasis/links/code lose their markers, code
/// fences are removed (their content is preserved), and Chinese text is kept.
enum PlainTextExtractor {

    /// Strips the YAML Front Matter when the document opens with a `---` fence.
    private static func withoutFrontMatter(_ markdown: String) -> String {
        let lines = markdown.components(separatedBy: .newlines)
        if let first = lines.first, first.trimmingCharacters(in: .whitespaces) == "---" {
            if let close = lines.dropFirst().firstIndex(where: {
                $0.trimmingCharacters(in: .whitespaces) == "---"
            }) {
                // Body is everything after the closing fence.
                return lines[(close + 1)...].joined(separator: "\n")
            }
        }
        return markdown
    }

    /// Squeezes repeated whitespace, keeping single spaces/newlines for FTS.
    static func plainText(from markdown: String) -> String {
        let body = withoutFrontMatter(markdown)
        var result = [String]()

        for raw in body.components(separatedBy: .newlines) {
            let line = stripInlineMarkdown(raw.trimmingCharacters(in: .whitespaces))
            // Drop empty lines but keep the structure otherwise.
            if line.isEmpty { result.append(""); continue }
            result.append(line)
        }

        // Rejoin and collapse runs of blank lines to at most one.
        let joined = result.joined(separator: "\n")
        let collapsed = joined.replacingOccurrences(
            of: "\n{2,}", with: "\n", options: .regularExpression)
        return collapsed.trimmingCharacters(in: .whitespacesAndNewlines) + "\n"
    }

    /// Removes inline Markdown tokens from a single non-empty line.
    static func stripInlineMarkdown(_ line: String) -> String {
        var s = line
        // Images:  ![alt](url) → alt
        s = s.replacingOccurrences(
            of: #"!\[([^\]]*)\]\([^)]*\)"#, with: "$1", options: .regularExpression)
        // Links:  [text](url) → text
        s = s.replacingOccurrences(
            of: #"\[([^\]]*)\]\([^)]*\)"#, with: "$1", options: .regularExpression)
        // Inline code and backticks: `x` → x
        s = s.replacingOccurrences(of: #"`"#, with: "", options: .literal)
        // Emphasis & strong — strip only *paired* Markdown delimiters, keeping
        // the inner text and leaving bare markers / prose-internal delimiters
        // untouched (e.g. `snake_case`, `2*3`, `C++`) (T110). The lookbehind /
        // lookahead stop delimiters adjacent to word chars/digits/underscore
        // from being treated as Markdown, which is what keeps Chinese+English
        // notes searchable without corrupting ordinary text.
        s = s.replacingOccurrences(of: #"(?<![A-Za-z0-9])\*\*([^*]+)\*\*(?![A-Za-z0-9])"#, with: "$1", options: .regularExpression)
        s = s.replacingOccurrences(of: #"(?<![A-Za-z0-9_])\*([^*]*)\*(?![A-Za-z0-9_])"#, with: "$1", options: .regularExpression)
        s = s.replacingOccurrences(of: #"(?<![A-Za-z0-9])__([^_]+)__(?![A-Za-z0-9])"#, with: "$1", options: .regularExpression)
        s = s.replacingOccurrences(of: #"(?<![A-Za-z0-9_])_([^_]+)_(?![A-Za-z0-9_])"#, with: "$1", options: .regularExpression)
        s = s.replacingOccurrences(of: #"(?<!~)~([^~]+)~(?!~)"#, with: "$1", options: .regularExpression)
        // Heading marker #, blockquote >, list bullets, ordered markers
        let stripped = s
            .replacingOccurrences(of: #"^#{1,6}\s+"#, with: "", options: .regularExpression)
            .replacingOccurrences(of: #"^>\s?"#, with: "", options: .regularExpression)
            .replacingOccurrences(of: #"^[-*+]\s+"#, with: "", options: .regularExpression)
            .replacingOccurrences(of: #"^\d+\.\s+"#, with: "", options: .regularExpression)
            .trimmingCharacters(in: .whitespaces)
        return stripped
    }
}