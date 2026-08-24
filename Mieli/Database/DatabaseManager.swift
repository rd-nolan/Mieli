import Foundation
import GRDB

/// Opens the workspace's SQLite index database (AGENTS §22, §23).
///
/// The database lives at `<workspace>/.mieli/index.sqlite` and is written via
/// GRDB. Opening creates `.mieli` idempotently, runs the schema migrations,
/// and configures WAL. This is a rebuildable local index, never user data.
enum DatabaseManager {

    /// The name of the SQLite index file inside `.mieli`.
    static let indexFileName = "index.sqlite"

    private static let currentInternalDirectoryName = ".mieli"
    private static let legacyInternalDirectoryName = ".muisti"
    private static let historicalInternalDirectoryName = ".minne"

    /// Opens (creating if needed) the `.mieli/index.sqlite` database and
    /// returns a GRDB `DatabaseQueue` bound to it.
    ///
    /// - Parameters:
    ///   - workspaceURL: the workspace root. `.mieli` is created idempotently.
    /// - Throws: an error if the directory cannot be created or the database
    ///   cannot be opened.
    static func openDatabaseQueue(at workspaceURL: URL) throws -> DatabaseQueue {
        let mieliDir = try prepareInternalDirectory(at: workspaceURL)

        let dbURL = mieliDir.appendingPathComponent(indexFileName)

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

    /// Moves a pre-Mieli index directory when the workspace has not yet
    /// been opened with the new name. If multiple legacy directories exist,
    /// the newest legacy name is preferred and the others are left untouched
    /// so no user data is silently discarded.
    private static func prepareInternalDirectory(at workspaceURL: URL) throws -> URL {
        let fileManager = FileManager.default
        let current = workspaceURL.appendingPathComponent(
            currentInternalDirectoryName, isDirectory: true)
        let legacyDirectories = [
            workspaceURL.appendingPathComponent(
                legacyInternalDirectoryName, isDirectory: true),
            workspaceURL.appendingPathComponent(
                historicalInternalDirectoryName, isDirectory: true)
        ]

        if !fileManager.fileExists(atPath: current.path),
           let legacy = legacyDirectories.first(where: {
               fileManager.fileExists(atPath: $0.path)
           }) {
            try fileManager.moveItem(at: legacy, to: current)
        }

        try fileManager.createDirectory(at: current, withIntermediateDirectories: true)
        return current
    }
}
