import Foundation

/// YAML Front Matter metadata for a Muisti note (AGENTS §9).
///
/// V1 metadata is deliberately minimal: `id`, `tags`, `created`, `updated`.
struct FrontMatter {
    var id: String?
    var tags: [String] = []
    var created: Date?
    var updated: Date?
}

/// Minimal Front Matter parser.
///
/// Parses the `---`-delimited header at the top of a Markdown document. Only
/// the four documented fields are extracted; an intentionally small custom
/// parser is used instead of a YAML dependency (AGENTS §48). It is tolerant:
/// missing or malformed fields are simply left as `nil`/empty rather than
/// failing the whole note.
enum FrontMatterParser {

    /// Extracts the Front Matter block.
    ///
    /// - Returns: a `FrontMatter` if a `---`…`---` header is present and
    ///   parses, `nil` if no header exists. `created`/`updated` are ISO-8601
    ///   strings; fields that are absent or malformed stay `nil`/empty.
    static func parse(_ markdown: String) -> FrontMatter? {
        let lines = markdown.components(separatedBy: .newlines)
        // The document must start with `---` (no leading blank lines allowed).
        guard let first = lines.first?.trimmingCharacters(in: .whitespaces),
              first == "---" else {
            return nil
        }

        // Find the closing `---` line.
        guard let closeIndex = lines.dropFirst().firstIndex(where: { line in
            line.trimmingCharacters(in: .whitespaces) == "---"
        }) else {
            // Missing closing fence: treat as malformed, no Front Matter.
            return nil
        }

        var matter = FrontMatter()
        let body = lines[1..<closeIndex]
        var currentTagsList = false

        for raw in body {
            let line = raw.trimmingCharacters(in: .whitespaces)
            guard !line.isEmpty else { continue }

            // `  - value` inside a `tags:` list.
            if currentTagsList, line.hasPrefix("-"), line != "-" {
                let value = line.dropFirst().trimmingCharacters(in: .whitespaces)
                if !value.isEmpty {
                    matter.tags.append(cleanTag(value))
                }
                continue
            }

            // Reset the in-list flag on any non-list key line.
            if line.contains(":") { currentTagsList = false }

            guard let colon = line.firstIndex(of: ":") else { continue }
            let key = line[..<colon].trimmingCharacters(in: .whitespaces)
            let rawValue = line[line.index(after: colon)...]
                .trimmingCharacters(in: .whitespaces)

            switch key {
            case "id":
                matter.id = rawValue.isEmpty ? nil : rawValue
            case "created":
                matter.created = parseDate(rawValue)
            case "updated":
                matter.updated = parseDate(rawValue)
            case "tags":
                if rawValue.hasPrefix("["), rawValue.hasSuffix("]") {
                    // Inline list: `tags: [a, b]`.
                    let inner = rawValue.dropFirst().dropLast()
                    for part in inner.split(separator: ",") {
                        let t = cleanTag(String(part))
                        if !t.isEmpty { matter.tags.append(t) }
                    }
                } else if rawValue.isEmpty {
                    // Block list follows on the next lines.
                    currentTagsList = true
                }
            default:
                break
            }
        }
        return matter
    }

    /// Strips leading `-`, quotes, and surrounding whitespace from a tag value.
    private static func cleanTag(_ value: String) -> String {
        value
            .replacingOccurrences(of: #"^['"]|['"]$"#, with: "", options: .regularExpression)
            .trimmingCharacters(in: .whitespaces)
    }

    private static let isoFormatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    /// Parses an ISO-8601 date string; nil for empty or malformed input.
    private static func parseDate(_ value: String) -> Date? {
        guard !value.isEmpty else { return nil }
        return isoFormatter.date(from: value)
    }
}