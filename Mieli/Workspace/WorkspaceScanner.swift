import Foundation

/// Recursively scans a Workspace directory into a tree of `WorkspaceItem`s.
///
/// Recognizes real directories and `*.md` files. Ignores Mieli-internal
/// directories (`.mieli`, legacy `.muisti`/`.minne`) and attachment folders
/// (`*.files`). Does not read or
/// index file contents — this is structure discovery only (T020).
enum WorkspaceScanner {

    /// Scans `root`, returning top-level items.
    ///
    /// `root` must exist and be a directory; throws otherwise.
    static func scan(_ root: URL) throws -> [WorkspaceItem] {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: root.path, isDirectory: &isDirectory),
              isDirectory.boolValue else {
            throw CocoaError(.fileReadNoSuchFile)
        }
        return try scanChildren(of: root, relativePrefix: "")
    }

    /// Returns the children of `dir` as items. `relativePrefix` is the parent
    /// folder's Workspace-relative path ("" at the workspace root); relative
    /// paths are accumulated here rather than derived by string prefix, which
    /// is robust to the `/var` → `/private/var` canonicalization
    /// `contentsOfDirectory` introduces.
    private static func scanChildren(of dir: URL, relativePrefix: String) throws -> [WorkspaceItem] {
        let contents = try FileManager.default.contentsOfDirectory(
            at: dir,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]
        )
        // `skipsHiddenFiles` also skips the internal directories; keep an
        // explicit guard anyway.
        return try contents
            .filter { !isIgnored($0) }
            .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
            .compactMap { try makeItem(for: $0, relativePrefix: relativePrefix) }
    }

    /// Builds an item for one entry. `relativePrefix` is its folder's path.
    /// Returns `nil` for non-directory files that are not Markdown.
    private static func makeItem(for url: URL, relativePrefix: String) throws -> WorkspaceItem? {
        var isDirectory: ObjCBool = false
        _ = FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory)

        let name = url.lastPathComponent
        let relative = relativePath(prefix: relativePrefix, name: name)

        if isDirectory.boolValue {
            return WorkspaceItem(
                name: name,
                kind: .folder,
                relativePath: relative,
                children: try scanChildren(of: url, relativePrefix: relative)
            )
        }
        guard url.pathExtension.lowercased() == "md" else { return nil }
        return WorkspaceItem(name: name, kind: .note, relativePath: relative)
    }

    private static func relativePath(prefix: String, name: String) -> String {
        prefix.isEmpty ? name : "\(prefix)/\(name)"
    }

    /// Directories to skip entirely: `.mieli`, legacy `.muisti`/`.minne`, and
    /// `*.files`.
    /// (attachment folders). Only directories match either rule.
    private static func isIgnored(_ url: URL) -> Bool {
        let name = url.lastPathComponent
        return name == ".mieli"
            || name == ".muisti"
            || name == ".minne"
            || name.hasSuffix(".files")
    }
}
