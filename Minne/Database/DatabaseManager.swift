import Foundation
import GRDB

/// Opens the workspace's SQLite index database (AGENTS §22, §23).
///
/// The database lives at `<workspace>/.minne/index.sqlite` and is written via
/// GRDB. Opening creates `.minne` idempotently, runs the schema migrations,
/// and configures WAL. This is a rebuildable local index, never user data.
enum DatabaseManager {

    /// The name of the SQLite index file inside `.minne`.
    static let indexFileName = "index.sqlite"

    /// Opens (creating if needed) the `.minne/index.sqlite` database and
    /// returns a GRDB `DatabaseQueue` bound to it.
    ///
    /// - Parameters:
    ///   - workspaceURL: the workspace root. `.minne` is created idempotently.
    /// - Throws: an error if the directory cannot be created or the database
    ///   cannot be opened.
    static func openDatabaseQueue(at workspaceURL: URL) throws -> DatabaseQueue {
        let minneDir = workspaceURL.appendingPathComponent(".minne")
        try FileManager.default.createDirectory(
            at: minneDir, withIntermediateDirectories: true)

        let dbURL = minneDir.appendingPathComponent(indexFileName)

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
}