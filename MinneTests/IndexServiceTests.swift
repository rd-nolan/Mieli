import XCTest
@testable import Minne
import GRDB

/// Verifies T045: Markdown → ParsedNote → SQLite (notes + tags + note_fts).
final class IndexServiceTests: XCTestCase {

    private var tempDir: URL!
    private var queue: DatabaseQueue!

    override func setUp() {
        super.setUp()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("minne-index-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        queue = try? DatabaseManager.openDatabaseQueue(at: tempDir)
    }

    override func tearDown() {
        try? queue?.close()
        try? FileManager.default.removeItem(at: tempDir)
        super.tearDown()
    }

    func testIndexesNoteMetadata() throws {
        let md = """
        ---
        id: 01K32M4PZXXXXXXXX
        tags: [Swift, 状态机]
        created: 2026-08-21T08:00:00+08:00
        ---
        # Swift 并发
        今天研究了 Spring 状态机实现方案
        """
        try IndexService.index(ParsedNote(relativePath: "工作/spring.md", markdown: md), in: queue)

        try queue.read { db in
            let path = try String.fetchOne(db, sql: "SELECT relative_path FROM notes WHERE id = '01K32M4PZXXXXXXXX'")!
            XCTAssertEqual(path, "工作/spring.md")
            let folder = try String.fetchOne(db, sql: "SELECT folder FROM notes WHERE id = '01K32M4PZXXXXXXXX'")!
            XCTAssertEqual(folder, "工作")
            let title = try String.fetchOne(db, sql: "SELECT title FROM notes WHERE id = '01K32M4PZXXXXXXXX'")!
            XCTAssertEqual(title, "Swift 并发")
            let created = try String.fetchOne(db, sql: "SELECT created_at FROM notes WHERE id = '01K32M4PZXXXXXXXX'")!
            XCTAssertEqual(created, "2026-08-21T00:00:00Z") // normalized ISO-8601 UTC
        }
    }

    func testIndexesTagsIntoTagAndLinkTables() throws {
        let md = "---\ntags: [Swift, 状态机]\n---\n# t"
        try IndexService.index(ParsedNote(relativePath: "a.md", markdown: md), in: queue)

        try queue.read { db in
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM tags")!, 2)
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_tags")!, 2)
            let names = try String.fetchAll(db, sql: "SELECT name FROM tags")
            XCTAssertEqual(Set(names), ["Swift", "状态机"])
        }
    }

    func testIndexEnablesSearchAcrossContentAndTags() throws {
        let md = """
        ---
        id: note1
        tags: [macOS]
        ---
        # Meta
        今天研究了实现方案
        """
        try IndexService.index(ParsedNote(relativePath: "meta.md", markdown: md), in: queue)

        try queue.read { db in
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_fts WHERE note_fts MATCH '实现方案'")!, 1)
            let matched = try Int.fetchOne(db, sql: """
                SELECT COUNT(*) FROM note_fts
                WHERE rowid = (SELECT rowid FROM notes WHERE id = 'note1')
                  AND note_fts MATCH 'macOS'
                """)!
            XCTAssertEqual(matched, 1, "tag token must be searchable via FTS")
        }
    }

    func testParsedNoteGeneratesStableIdWhenMissing() throws {
        let note = ParsedNote(relativePath: "无id.md", markdown: "# 标题\n内容")
        XCTAssertEqual(note.id.count, 26)  // fresh ULID
        XCTAssertEqual(note.title, "标题")
    }

    func testParsedNoteUsesFrontMatterIdAndFilenameFallback() throws {
        let md = """
        ---
        id: fixedID1
        ---
        无标题内容
        """
        let note = ParsedNote(relativePath: "归档/说明.md", markdown: md)
        XCTAssertEqual(note.id, "fixedID1")
        XCTAssertEqual(note.title, "说明") // falls back to filename
        XCTAssertEqual(note.folder, "归档")
    }

    func testIndexIsTransactionalOnFailure() throws {
        // Pre-seed a conflicting relative_path so the notes insert fails.
        try queue.write { db in
            try db.execute(sql: """
                INSERT INTO notes (id, relative_path, filename, title, folder)
                VALUES ('x', 'dup.md', 'dup.md', 'X', '')
                """)
        }
        let dup = ParsedNote(relativePath: "dup.md", markdown: "# 新内容")
        try? IndexService.index(dup, in: queue) // duplicate relative_path → fails

        try queue.read { db in
            // No orphan FTS row left behind by the rolled-back transaction.
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_fts")!, 0)
        }
    }

    // MARK: - T046 update

    /// Seeds a note whose Front Matter carries a fixed id.
    private func seedNote(relativePath: String, id: String, tags: [String], markdown: String) throws {
        let fmTags = tags.map { "- \($0)\n" }.joined()
        let md = "---\nid: \(id)\ntags:\n\(fmTags)---\n\n\(markdown)"
        try IndexService.index(ParsedNote(relativePath: relativePath, markdown: md), in: queue)
    }

    func testUpdateRevisesMetadataAndFTS() throws {
        try seedNote(relativePath: "a.md", id: "n1", tags: ["Swift"], markdown: "# 旧标题\n旧内容")
        try IndexService.update(ParsedNote(relativePath: "a.md", markdown: """
            ---
            id: n1
            tags:
              - Swift
            ---
            # 新标题
            新的正文内容
            """), in: queue)

        try queue.read { db in
            let title = try String.fetchOne(db, sql: "SELECT title FROM notes WHERE id = 'n1'")!
            XCTAssertEqual(title, "新标题")
            // FTS reflects the new content; the old token is gone.
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_fts WHERE note_fts MATCH '新标题'")!, 1)
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_fts WHERE note_fts MATCH '旧标题'")!, 0)
        }
    }

    func testUpdateReconcilesTagLinks() throws {
        try seedNote(relativePath: "a.md", id: "n1", tags: ["Swift", "旧"], markdown: "# t")
        try IndexService.update(ParsedNote(relativePath: "a.md", markdown: """
            ---
            id: n1
            tags:
              - Swift
              - 新
            ---
            # t
            """), in: queue)

        try queue.read { db in
            // 2 links remain (Swift kept, 新 added, 旧 removed).
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_tags WHERE note_id = 'n1'")!, 2)
            let linked = Set(try String.fetchAll(db, sql: """
                SELECT t.name FROM tags t
                JOIN note_tags nt ON nt.tag_id = t.id
                WHERE nt.note_id = 'n1'
                """))
            XCTAssertEqual(linked, ["Swift", "新"])
            XCTAssertFalse(linked.contains("旧"))
        }
    }

    func testUpdateThrowsWhenNoteNotIndexed() throws {
        let note = ParsedNote(relativePath: "ghost.md", markdown: "# 不存在")
        XCTAssertThrowsError(try IndexService.update(note, in: queue)) { error in
            XCTAssertEqual(error as? IndexService.IndexServiceError, .noteNotIndexed)
        }
    }

    func testUpdateKeepsStableIdAcrossContentChange() throws {
        try seedNote(relativePath: "a.md", id: "fixed-x", tags: [], markdown: "# 一")
        try IndexService.update(ParsedNote(relativePath: "a.md", markdown: """
            ---
            id: fixed-x
            ---
            # 二
            """), in: queue)
        // Only one row; the id did not change.
        XCTAssertEqual(try queue.read { try Int.fetchOne($0, sql: "SELECT COUNT(*) FROM notes") }, 1)
    }

    // MARK: - T047 remove

    func testRemoveClearsAllThreeStores() throws {
        try seedNote(relativePath: "工作/a.md", id: "n1", tags: ["Swift"], markdown: "# 标题\n正文")
        try IndexService.remove(relativePath: "工作/a.md", in: queue)

        try queue.read { db in
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM notes WHERE id = 'n1'")!, 0)
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_tags WHERE note_id = 'n1'")!, 0)
            XCTAssertEqual(try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_fts")!, 0)
        }
    }

    func testRemoveLeavesOtherNotesIntact() throws {
        try seedNote(relativePath: "a.md", id: "n1", tags: ["Swift"], markdown: "# 甲")
        try seedNote(relativePath: "b.md", id: "n2", tags: ["Swift"], markdown: "# 乙")
        try IndexService.remove(relativePath: "a.md", in: queue)

        let remainingCount: Int? = try queue.read { db in
            try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM notes")
        }
        let remainingID: String? = try queue.read { db in
            try String.fetchOne(db, sql: "SELECT id FROM notes")
        }
        let ftsCount: Int? = try queue.read { db in
            try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_fts")
        }
        XCTAssertEqual(remainingCount, 1)
        XCTAssertEqual(remainingID, "n2")
        XCTAssertEqual(ftsCount, 1)
    }

    func testRemoveIsIdempotent() throws {
        // Removing a never-indexed path must not throw.
        XCTAssertNoThrow(try IndexService.remove(relativePath: "ghost.md", in: queue))
    }

    // MARK: - T048 rebuild

    private func writeFile(_ name: String, _ content: String) throws {
        let url = tempDir.appendingPathComponent(name)
        try content.data(using: .utf8)!.write(to: url)
    }

    func testRebuildPopulatesIndexFromWorkspace() throws {
        try writeFile("a.md", "---\nid: rb-a\ntags:\n- Swift\n---\n# 甲\n正文内容")
        try writeFile("b.md", "---\nid: rb-b\n---\n# 乙\n另一个实现方案")

        try IndexRebuilder.rebuild(workspaceURL: tempDir, in: queue)

        let count: Int? = try queue.read { db in try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM notes") }
        let ftsMatchHits: Int? = try queue.read { db in
            try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_fts WHERE note_fts MATCH '实现方案'")
        }
        let titleA: String? = try queue.read { db in
            try String.fetchOne(db, sql: "SELECT title FROM notes WHERE id = 'rb-a'")
        }
        XCTAssertEqual(count, 2)
        XCTAssertEqual(ftsMatchHits, 1)
        XCTAssertEqual(titleA, "甲")
    }

    func testRebuildDoesNotModifyMarkdown() throws {
        let original = "---\nid: rb-x\ntags: [Swift]\n---\n# 唯一\n内容不变"
        try writeFile("keep.md", original)

        try IndexRebuilder.rebuild(workspaceURL: tempDir, in: queue)

        let after = try String(contentsOf: tempDir.appendingPathComponent("keep.md"), encoding: .utf8)
        XCTAssertEqual(after, original)
    }

    func testRebuildReplacesStaleIndex() throws {
        // Index a note whose file then disappears → rebuild must drop it.
        try IndexService.index(ParsedNote(relativePath: "gone.md", markdown: "# 已删"), in: queue)
        try writeFile("live.md", "# 保留")

        try IndexRebuilder.rebuild(workspaceURL: tempDir, in: queue)

        let count: Int? = try queue.read { db in try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM notes") }
        let liveTitle: String? = try queue.read { db in
            try String.fetchOne(db, sql: "SELECT title FROM notes WHERE filename = 'live.md'")
        }
        XCTAssertEqual(count, 1)
        XCTAssertEqual(liveTitle, "保留")
    }

    // MARK: - T049 incremental startup reconcile

    private func setModifiedDate(_ name: String) throws {
        try FileManager.default.setAttributes(
            [.modificationDate: Date()],
            ofItemAtPath: tempDir.appendingPathComponent(name).path)
    }

    func testReconcileIndexesNewFiles() throws {
        try writeFile("a.md", "---\nid: r-a\n---\n# 甲\n正文")
        let sub = tempDir.appendingPathComponent("子")
        try FileManager.default.createDirectory(at: sub, withIntermediateDirectories: true)
        try writeFile("子/b.md", "---\nid: r-b\n---\n# 乙\n第二个文件")

        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        let count: Int? = try queue.read { db in try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM notes") }
        let mtimeStored: Double? = try queue.read { db in
            try Double.fetchOne(db, sql: "SELECT file_mtime FROM notes WHERE id = 'r-a'")
        }
        XCTAssertEqual(count, 2)
        XCTAssertNotNil(mtimeStored) // fs metadata written into the row
    }

    func testReconcileSkipsUnchangedOnSecondRun() throws {
        try writeFile("a.md", "---\nid: r-a\n---\n# 甲\n正文")
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)
        let m1: Double? = try queue.read { db in
            try Double.fetchOne(db, sql: "SELECT file_mtime FROM notes WHERE id = 'r-a'")
        }

        // Second run with nothing changed: same count, same mtime (no reindex).
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)
        let count: Int? = try queue.read { db in try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM notes") }
        let m2: Double? = try queue.read { db in
            try Double.fetchOne(db, sql: "SELECT file_mtime FROM notes WHERE id = 'r-a'")
        }
        XCTAssertEqual(count, 1)
        XCTAssertEqual(m1, m2)
    }

    func testReconcileReindexesModifiedFile() throws {
        try writeFile("a.md", "---\nid: r-a\n---\n# 旧标题\n旧内容")
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        try writeFile("a.md", "---\nid: r-a\n---\n# 新标题\n新正文")
        try setModifiedDate("a.md")
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        let title: String? = try queue.read { db in
            try String.fetchOne(db, sql: "SELECT title FROM notes WHERE id = 'r-a'")
        }
        let ftsHit: Int? = try queue.read { db in
            try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_fts WHERE note_fts MATCH '新正文'")
        }
        XCTAssertEqual(title, "新标题")
        XCTAssertEqual(ftsHit, 1)
    }

    func testReconcileRemovesDeletedFile() throws {
        try writeFile("a.md", "---\nid: r-a\n---\n# 甲")
        try writeFile("b.md", "---\nid: r-b\n---\n# 乙")
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        try FileManager.default.removeItem(at: tempDir.appendingPathComponent("a.md"))
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        let remaining: Int? = try queue.read { db in try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM notes") }
        let ids = try queue.read { db in try String.fetchAll(db, sql: "SELECT id FROM notes") }
        XCTAssertEqual(remaining, 1)
        XCTAssertEqual(ids, ["r-b"])
    }

    /// T106: reconcile's deleted-file path must clear the FTS5 row too, not
    /// just the `notes` row. `note_fts` is an independent table (no FK
    /// cascade), so leaving it behind produces an orphan FTS row — searchable,
    /// and later mis-associated with a new note that reuses the freed rowid.
    func testReconcileRemovesDeletedFileAlsoClearsFTS() throws {
        try writeFile("a.md", "---\nid: r-a\n---\n# 甲\n一个独特词汇甲")
        try writeFile("b.md", "---\nid: r-b\n---\n# 乙\n独自残留乙")
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        // Sanity: both notes are searchable before the external delete.
        let before = try SearchService.search("独特词汇甲", in: queue)
        XCTAssertEqual(before.map(\.id), ["r-a"])

        try FileManager.default.removeItem(at: tempDir.appendingPathComponent("a.md"))
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        // The deleted note's FTS row must be gone, not merely hidden by a JOIN.
        let ftsRows: Int? = try queue.read { db in
            try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM note_fts")
        }
        XCTAssertEqual(ftsRows, 1, "orphan FTS row for the deleted note must be removed")

        // Its content must no longer be searchable.
        let hits = try SearchService.search("独特词汇甲", in: queue)
        XCTAssertTrue(hits.isEmpty, "deleted note content must not appear in search")

        // The surviving note's own FTS row is intact (multi-char query for the
        // trigram tokenizer, which cannot match single-character input).
        let survivor = try SearchService.search("独自残留", in: queue)
        XCTAssertEqual(survivor.map(\.id), ["r-b"])
    }

    // MARK: - T066 updateFile (single-note index refresh after save)

    func testUpdateFileReflectsEditedContent() throws {
        try writeFile("note.md", "---\nid: u1\n---\n# 标题\n\n旧词时间\n")
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        // The user saves edited content (simulates FileService.write).
        try writeFile("note.md", "---\nid: u1\n---\n# 标题\n\n新词正好出现\n")
        try IndexUpdater.updateFile(at: "note.md", workspace: tempDir, in: queue)

        let hitsNew = try SearchService.search("新词正好", in: queue)
        let hitsOld = try SearchService.search("旧词时间", in: queue)
        XCTAssertTrue(hitsNew.contains { $0.relativePath == "note.md" })
        XCTAssertFalse(hitsOld.contains { $0.relativePath == "note.md" })
    }

    func testUpdateFileRefreshMetadataSoNextReconcileSkips() throws {
        try writeFile("note.md", "---\nid: u2\n---\n# 甲\n")
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        // Save a change; updateFile rewrites mtime/size/hash to match disk.
        try writeFile("note.md", "---\nid: u2\n---\n# 甲\n\n更多内容\n")
        try IndexUpdater.updateFile(at: "note.md", workspace: tempDir, in: queue)

        // A subsequent reconcile sees the file as unchanged → still one row,
        // and the stored mtime now matches what disk will report.
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)
        let count: Int? = try queue.read { db in try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM notes") }
        XCTAssertEqual(count, 1)

        let (mtime, hash): (Double, String) = try queue.read { db in
            guard let row = try Row.fetchOne(db, sql: "SELECT file_mtime, content_hash FROM notes WHERE relative_path = 'note.md'") else {
                throw TestError.missingRow
            }
            let mt = row["file_mtime"] as? Double
            let hs = row["content_hash"] as? String
            return (mt ?? -1, hs ?? "")
        }
        XCTAssertGreaterThanOrEqual(mtime, 0)
        XCTAssertFalse(hash.isEmpty)
    }

    // MARK: - T107 stable id for notes missing Front Matter id

    /// An externally created note without Front Matter `id` must keep its id
    /// stable across edits. Before the fix, `ParsedNote` minted a fresh ULID on
    /// every re-parse, so `update` missed the row and the index never refreshed.
    func testUpdateFileSucceedsForIdlessNoteKeepingStableId() throws {
        // A note with no Front Matter id, indexed by reconcile the first time.
        try writeFile("note.md", "# 旧标题\n一个旧词汇甲\n")
        try IndexUpdater.reconcile(workspace: tempDir, in: queue)

        let idBefore: String = try queue.read { db in
            try String.fetchOne(db, sql: "SELECT id FROM notes WHERE relative_path = 'note.md'")!
        }

        // External edit: change the content. `updateFile` must update the same
        // row (preserved id), not throw or insert a duplicate.
        try writeFile("note.md", "# 新标题\n一个新词汇乙\n")
        try IndexUpdater.updateFile(at: "note.md", workspace: tempDir, in: queue)

        let idAfter: String = try queue.read { db in
            try String.fetchOne(db, sql: "SELECT id FROM notes WHERE relative_path = 'note.md'")!
        }
        XCTAssertEqual(idAfter, idBefore, "id-less note id must stay stable across edit")

        // Exactly one row (no duplicate insert from an id change).
        let rows: Int? = try queue.read { db in
            try Int.fetchOne(db, sql: "SELECT COUNT(*) FROM notes WHERE relative_path = 'note.md'")
        }
        XCTAssertEqual(rows, 1)

        // New content is searchable; stale content is gone.
        let hitsNew = try SearchService.search("新词汇乙", in: queue)
        XCTAssertTrue(hitsNew.contains { $0.relativePath == "note.md" })
        let hitsOld = try SearchService.search("旧词甲", in: queue)
        XCTAssertFalse(hitsOld.contains { $0.relativePath == "note.md" })
    }

    /// ParsedNote with the same `existingStableID` is idempotent across parses.
    func testParsedNoteKeepsExistingIdWhenFrontMatterMissing() throws {
        let first = ParsedNote(relativePath: "x.md", markdown: "# a",
                               existingStableID: "persisted-abc")
        let second = ParsedNote(relativePath: "x.md", markdown: "# a\n更多",
                                existingStableID: "persisted-abc")
        XCTAssertEqual(first.id, "persisted-abc")
        XCTAssertEqual(second.id, "persisted-abc")
    }

    /// A Front Matter id overrides any stored/existing id (it is authoritative).
    func testFrontMatterIdWinsOverExistingStableID() throws {
        let note = ParsedNote(relativePath: "x.md",
                              markdown: "---\nid: FM\n---\n# t",
                              existingStableID: "stored-old")
        XCTAssertEqual(note.id, "FM")
    }

    private enum TestError: Error { case missingRow }
}