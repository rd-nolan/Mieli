import Foundation

/// A compact note identity used when showing a list of notes that share a tag
/// (T074). Mirrors the essential fields of a scanned tree item but is
/// workspace-index-derived rather than a file-system scan node.
struct TaggedNote: Identifiable, Hashable {
    let id: String
    let filename: String
    let title: String
    let folder: String      // "" == workspace root
    let relativePath: String
}

/// A node in a scanned Workspace: either a folder (real directory) or a
/// Markdown note. Not a virtual hierarchy — mirrors the on-disk layout.
///
/// Paths are Workspace-relative strings (no leading slash), per AGENTS.md §23.
struct WorkspaceItem: Identifiable, Hashable {
    enum Kind: Equatable, Hashable {
        case folder
        case note
    }

    /// Stable identity within a scan; the relative path is unique per item.
    var id: String { relativePath }

    let name: String
    let kind: Kind
    let relativePath: String
    /// Child items for a `.folder`; `nil` for a `.note`.
    var children: [WorkspaceItem]?
}