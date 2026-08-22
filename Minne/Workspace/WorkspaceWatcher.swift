import Foundation

/// Kind of a single workspace-relative file change (T090).
enum WorkspaceChangeKind {
    case created, modified, deleted, renamed
}

/// A workspace-relative path change observed by the watcher.
struct WorkspaceChange {
    /// Workspace-relative path, posix separators (e.g. `工作/方案.md`).
    let path: String
    let kind: WorkspaceChangeKind
}

/// Watches a local Workspace directory for filesystem changes (T090).
///
/// Uses a lightweight **scan + diff** loop: periodically re-scans the
/// workspace (the same traversal the scanner uses, which already ignores
/// `.minne` and `*.files` per AGENTS §2/§20) and reports Markdown files that
/// were added, removed, or whose size/mtime changed.
///
/// This is a local observer only — it does not edit the index or the UI, and
/// it must never become a sync engine. Consumers decide how to react (index
/// update in T091+, tree refresh, reload of an open note).
///
/// Renames/moves are not distinguishable from delete+create at this layer and
/// are surfaced as such; pair-resolution lives in T094's consumer.
///
/// Lifecycle: `start(root:)` begins the polling loop; `stop()` cancels it.
/// The watch interval is public so tests can drive it.
final class WorkspaceWatcher {

    /// Poll interval. Small for snappy UI echo; keeps polling cheap on the
    /// modest note counts a V1 workspace holds.
    var interval: TimeInterval = 1.0

    private var root: URL?
    private var task: Task<Void, Never>?
    private var snapshot: [String: FileStamp] = [:]

    /// Called on the main actor with a batch of workspace-relative changes.
    var onChanges: (([WorkspaceChange]) -> Void)?

    init() {}

    deinit { stop() }

    func start(root: URL) {
        guard task == nil else { return }
        self.root = root
        // Establish the baseline snapshot without emitting "changed" noise.
        snapshot = Self.workspaceSnapshot(root: root) ?? [:]
        task = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: UInt64((self?.interval ?? 1.0) * 1_000_000_000))
                if Task.isCancelled { break }
                self?.tick()
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
        root = nil
        snapshot = [:]
    }

    private func tick() {
        guard let root else { return }
        guard let current = WorkspaceWatcher.workspaceSnapshot(root: root) else { return }
        let changes = WorkspaceWatcher.diff(new: current, old: snapshot)
        snapshot = current
        guard !changes.isEmpty else { return }
        onChanges?(changes)
    }

    // MARK: snapshot + diff (pure, testable)

    struct FileStamp: Equatable {
        let size: Int64
        let mtime: Int64
    }

    /// Scans `root` for Markdown files (ignoring `.minne`/`*.files`) returning
    /// relativePath → file stamp. `nil` when the root is unavailable.
    static func workspaceSnapshot(root: URL) -> [String: FileStamp]? {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: root.path, isDirectory: &isDirectory),
              isDirectory.boolValue else { return nil }
        var result: [String: FileStamp] = [:]
        walk(root, relativePrefix: "", into: &result)
        return result
    }

    private static func walk(_ dir: URL,
                             relativePrefix: String,
                             into out: inout [String: FileStamp]) {
        let contents = (try? FileManager.default.contentsOfDirectory(
            at: dir, includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles])) ?? []
        for url in contents {
            let name = url.lastPathComponent
            if name == ".minne" || name.hasSuffix(".files") { continue }
            let rel = relativePrefix.isEmpty ? name : "\(relativePrefix)/\(name)"
            var isDir: ObjCBool = false
            _ = FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir)
            if isDir.boolValue {
                walk(url, relativePrefix: rel, into: &out)
            } else if url.pathExtension.lowercased() == "md",
                      let attrs = try? FileManager.default.attributesOfItem(atPath: url.path) {
                guard let size = (attrs[.size] as? NSNumber)?.int64Value,
                      let mtime = (attrs[.modificationDate] as? Date)?.timeIntervalSince1970 else { continue }
                out[rel] = FileStamp(size: size, mtime: Int64(mtime))
            }
        }
    }

    /// Pure: diff two snapshots into ordered, workspace-relative changes.
    /// Added → created; removed → deleted; changed stamp → modified.
    static func diff(new: [String: FileStamp], old: [String: FileStamp]) -> [WorkspaceChange] {
        var changes: [WorkspaceChange] = []
        for (path, stamp) in new where old[path] == nil {
            changes.append(WorkspaceChange(path: path, kind: .created))
            _ = stamp
        }
        for (path, stamp) in old where new[path] == nil {
            changes.append(WorkspaceChange(path: path, kind: .deleted))
            _ = stamp
        }
        for (path, stamp) in new where old[path] != nil && old[path] != stamp {
            changes.append(WorkspaceChange(path: path, kind: .modified))
        }
        return changes
    }

    /// Whether a workspace-relative path is scan-internal/attachment and thus
    /// out of scope (same rule as the scanner).
    static func isIgnoredPath(_ relativePath: String) -> Bool {
        for segment in relativePath.split(separator: "/") {
            let s = String(segment)
            if s == ".minne" || s.hasSuffix(".files") { return true }
        }
        return false
    }
}