import Foundation
import GRDB

/// Opens the workspace's SQLite index database (AGENTS §22, §23).
///
/// The database lives at `<workspace>/.muisti/index.sqlite` and is written via
/// GRDB. Opening creates `.muisti` idempotently, runs the schema migrations,
/// and configures WAL. This is a rebuildable local index, never user data.
enum DatabaseManager {

    /// The name of the SQLite index file inside `.muisti`.
    static let indexFileName = "index.sqlite"

    private static let currentInternalDirectoryName = ".muisti"
    private static let legacyInternalDirectoryName = ".minne"

    /// Opens (creating if needed) the `.muisti/index.sqlite` database and
    /// returns a GRDB `DatabaseQueue` bound to it.
    ///
    /// - Parameters:
    ///   - workspaceURL: the workspace root. `.muisti` is created idempotently.
    /// - Throws: an error if the directory cannot be created or the database
    ///   cannot be opened.
    static func openDatabaseQueue(at workspaceURL: URL) throws -> DatabaseQueue {
        let muistiDir = try prepareInternalDirectory(at: workspaceURL)

        let dbURL = muistiDir.appendingPathComponent(indexFileName)

        var configuration = Configuration()
        // WAL: safer concurrent reads/writes on macOS and keeps FTS happy.
        configuration.prepareDatabase { db in
            try db.execute(sql: "PRAGMA journal_mode = WAL")
        }
        configuration.busyMode = .timeout(1.0)

        let queue = try DatabaseQueue(path: dbURL.path, configuration: configuration)
        // Create the current schema (idempotent via GRDB migrations).
        try Schema.makeMigrator().migrate(queue)
        return queue
    }

    /// Moves the pre-Muisti index directory when the workspace has not yet
    /// been opened with the new name. If both directories exist, the legacy
    /// one is left untouched so no user data is silently discarded.
    private static func prepareInternalDirectory(at workspaceURL: URL) throws -> URL {
        let fileManager = FileManager.default
        let current = workspaceURL.appendingPathComponent(
            currentInternalDirectoryName, isDirectory: true)
        let legacy = workspaceURL.appendingPathComponent(
            legacyInternalDirectoryName, isDirectory: true)

        if !fileManager.fileExists(atPath: current.path),
           fileManager.fileExists(atPath: legacy.path) {
            try fileManager.moveItem(at: legacy, to: current)
        }

        try fileManager.createDirectory(at: current, withIntermediateDirectories: true)
        return current
    }
}
