import Foundation

/// Reads a note's tags from its YAML Front Matter (AGENTS §9, T034).
///
/// This is a thin, named domain read built on `FrontMatterParser` (T030) —
/// consumed later by the sidebar, search, and index. Tags that are absent or
/// malformed yield an empty list, never a failure.
enum NoteTags {

    /// Resolves inline tag input to one canonical spelling (T126).
    /// Current-note spelling wins so a case-only duplicate remains a no-op;
    /// otherwise reuse an exact Workspace spelling, then a deterministic
    /// case-insensitive match before treating the trimmed input as new.
    static func resolveTag(
        _ input: String,
        currentTags: [String],
        workspaceTags: [String]
    ) -> String? {
        let trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }

        if let current = currentTags.first(where: {
            $0.compare(trimmed, options: [.caseInsensitive]) == .orderedSame
        }) {
            return current
        }
        if let exact = workspaceTags.first(where: { $0 == trimmed }) {
            return exact
        }
        let matches = workspaceTags.filter {
            $0.compare(trimmed, options: [.caseInsensitive]) == .orderedSame
        }
        return matches.sorted().first ?? trimmed
    }

    /// Returns the tags declared in a note's Front Matter, or `[]` when there
    /// is no Front Matter (or no `tags` key).
    static func tags(in markdown: String) -> [String] {
        FrontMatterParser.parse(markdown)?.tags ?? []
    }

    /// Returns a new Markdown string with `tag` appended to the Front Matter
    /// `tags` list (T071). No-op (idempotent) when the tag already exists or is
    /// blank. Preserves all body content and unrelated Front Matter keys.
    ///
    /// - A Front Matter block exists: appends `- <tag>` under `tags:`, or
    ///   inserts a `tags:` block right after the opening `---` when absent.
    /// - No Front Matter: prepends a minimal `tags:`-only block.
    static func addTag(_ tag: String, to markdown: String) -> String {
        let trimmed = tag.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return markdown }

        let lines = markdown.components(separatedBy: "\n")
        // No opening `---` at the very start → there is no Front Matter.
        guard let first = lines.first?.trimmingCharacters(in: .whitespaces),
              first == "---" else {
            return "---\ntags:\n  - \(trimmed)\n---\n" + markdown
        }

        guard let close = lines.indices.dropFirst()
            .first(where: { lines[$0].trimmingCharacters(in: .whitespaces) == "---" }) else {
            return markdown // malformed block: leave untouched
        }
        if tags(in: markdown).contains(trimmed) { return markdown } // already tagged

        var new = lines
        if let tagsIndex = (1..<close).first(where: {
            lines[$0].trimmingCharacters(in: .whitespaces).hasPrefix("tags:")
        }) {
            let tagsLine = lines[tagsIndex].trimmingCharacters(in: .whitespaces)
            let inlineValue = String(tagsLine.dropFirst("tags:".count))
                .trimmingCharacters(in: .whitespaces)
            if !inlineValue.isEmpty {
                // New notes use `tags: []`, and imported notes may use an
                // inline array. Convert that representation to one valid YAML
                // block list before appending; mixing both makes the parser
                // keep seeing the old inline value and hides the new tag.
                let updatedTags = tags(in: markdown) + [trimmed]
                new[tagsIndex] = "tags:"
                new.insert(contentsOf: updatedTags.map { "  - \($0)" }, at: tagsIndex + 1)
                return new.joined(separator: "\n")
            }
            // Append after the last contiguous list item under `tags:` so order
            // is preserved (a bare "after tags:" insert would land *before*
            // the first existing item).
            var insertAt = tagsIndex + 1
            while insertAt < close,
                  new[insertAt].trimmingCharacters(in: .whitespaces).hasPrefix("-") {
                insertAt += 1
            }
            new.insert("  - \(trimmed)", at: insertAt)
        } else {
            // No tags key yet: add it right after the opening `---`.
            new.insert("tags:", at: 1)
            new.insert("  - \(trimmed)", at: 2)
        }
        return new.joined(separator: "\n")
    }

    /// Returns a new Markdown string with `tag` removed from the Front Matter
    /// `tags` list (T072). No-op when the tag isn't present or is blank;
    /// the resulting empty `tags:` list is dropped. Body content of the note —
    /// and any list items in the body — are preserved.
    static func removeTag(_ tag: String, from markdown: String) -> String {
        let trimmed = tag.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return markdown }
        guard tags(in: markdown).contains(trimmed) else { return markdown }

        let target = "- \(trimmed)"
        let lines = markdown.components(separatedBy: "\n")
        // Clip to the Front Matter block.
        guard let close = lines.indices.dropFirst()
            .first(where: { lines[$0].trimmingCharacters(in: .whitespaces) == "---" }) else {
            return markdown // malformed block
        }

        var out: [String] = []
        var listRemaining = 0
        for (idx, line) in lines.enumerated() {
            let t = line.trimmingCharacters(in: .whitespaces)
            // A Markdown list item inside the FM block (not the body).
            if (1...close).contains(idx), t.hasPrefix("- ") {
                if t == target { continue }   // drop the targeted tag
                listRemaining += 1
            }
            out.append(line)
        }
        // List now empty → also drop the `tags:` key (no stray empty list).
        if listRemaining == 0 {
            out = out.filter { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("tags:") }
        }
        return out.joined(separator: "\n")
    }
}
