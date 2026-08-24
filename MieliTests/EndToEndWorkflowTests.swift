import XCTest
@testable import Mieli
import GRDB

/// Comprehensive End-to-End (E2E) test suite verifying all core user journeys:
/// 1. Workspace initialization, bookmark creation/persistence/restore, `.mieli/` isolation.
/// 2. Note creation with ULID front matter, H1/filename title parsing, and atomic save.
/// 3. Folder creation, renaming, note moving across folders, and directory tree scanning.
/// 4. Attachments pipeline: bundle creation (`.files/`), collision handling (`-1`), relative links, and note rename with folder + link rewriting.
/// 5. Tag management: adding/removing front matter tags, workspace tag queries, and filtering.
/// 6. Full-Text Search: SQLite FTS5 trigram, Chinese/English tokenization, ranking (title > content > path), snippets, incremental update, and full rebuild.
/// 7. External filesystem changes: watcher observation, external creations, own write suppression, and conflict detection.
/// 8. Editor-backed tag and rename mutations preserve the latest Markdown.
final class EndToEndWorkflowTests: XCTestCase {

    private var workspaceURL: URL!
    private var bookmarkFileURL: URL!
    private var workspaceManager: WorkspaceManager!

    @MainActor
    override func setUp() async throws {
        try await super.setUp()
        let tempRoot = FileManager.default.temporaryDirectory
            .appendingPathComponent("mieli-e2e-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempRoot, withIntermediateDirectories: true)
        workspaceURL = tempRoot
        bookmarkFileURL = tempRoot.appendingPathComponent("mieli.bookmark")
        workspaceManager = WorkspaceManager()
    }

    @MainActor
    override func tearDown() async throws {
        workspaceManager = nil
        if let workspaceURL {
            try? FileManager.default.removeItem(at: workspaceURL)
        }
        workspaceURL = nil
        bookmarkFileURL = nil
        try await super.tearDown()
    }

    // MARK: - E2E Scenario 1: Workspace Lifecycle & Security-Scoped Bookmark

    @MainActor
    func testWorkspaceLifecycleAndBookmarkRestore() throws {
        // 1. Initialize .mieli directory
        let mieliDir = workspaceURL.appendingPathComponent(".mieli", isDirectory: true)
        try FileManager.default.createDirectory(at: mieliDir, withIntermediateDirectories: true)
        XCTAssertTrue(FileManager.default.fileExists(atPath: mieliDir.path))

        // 2. Create and persist bookmark
        let bookmarkData = try XCTUnwrap(workspaceManager.createBookmark(for: workspaceURL))
        XCTAssertTrue(workspaceManager.persist(bookmarkData, to: bookmarkFileURL))
        XCTAssertTrue(FileManager.default.fileExists(atPath: bookmarkFileURL.path))

        // 3. Restore workspace from bookmark
        let restoredURL = workspaceManager.restoreWorkspace(bookmarkSource: bookmarkFileURL)
        XCTAssertEqual(restoredURL?.standardizedFileURL, workspaceURL.standardizedFileURL)
        XCTAssertEqual(workspaceManager.workspaceURL?.standardizedFileURL, workspaceURL.standardizedFileURL)
        XCTAssertNotNil(workspaceManager.databaseQueue)
        XCTAssertNotNil(workspaceManager.mieliDirectoryURL)
    }

    // MARK: - E2E Scenario 2: Note Creation, Title, Front Matter, and Atomic Save

    @MainActor
    func testNoteCreationMetadataAndAtomicSave() throws {
        _ = workspaceManager.restoreWorkspace(bookmarkSource: bookmarkFileURL)
        let noteRelPath = "工作/技术方案.md"
        let noteDir = workspaceURL.appendingPathComponent("工作", isDirectory: true)
        try FileManager.default.createDirectory(at: noteDir, withIntermediateDirectories: true)

        let initialMD = """
        ---
        id: 01HZZZZZZZZZZZZZZZZZZZZZZZ
        tags:
          - Swift
          - Architecture
        created: 2026-08-23T10:00:00+08:00
        updated: 2026-08-23T10:00:00+08:00
        ---
        # Mieli 架构与状态机设计

        这里是正文内容，深入讨论了 Swift 并发与状态机的实现方案。
        """

        let fileURL = workspaceURL.appendingPathComponent(noteRelPath)
        try FileService.saveMarkdown(initialMD, to: fileURL)

        // 1. Verify file exists on disk and content matches
        XCTAssertTrue(FileManager.default.fileExists(atPath: fileURL.path))
        let loaded = try String(contentsOf: fileURL, encoding: .utf8)
        XCTAssertEqual(loaded, initialMD)

        // 2. Parse metadata
        let parsedFM = FrontMatterParser.parse(loaded)
        XCTAssertEqual(parsedFM?.id, "01HZZZZZZZZZZZZZZZZZZZZZZZ")
        XCTAssertEqual(parsedFM?.tags, ["Swift", "Architecture"])

        // 3. Parse title: H1 precedence
        let title = NoteTitleParser.title(of: loaded, filename: "技术方案.md")
        XCTAssertEqual(title, "Mieli 架构与状态机设计")

        // 4. Test filename fallback if H1 is missing
        let noH1MD = "---\nid: test\n---\n无标题内容"
        let fallbackTitle = NoteTitleParser.title(of: noH1MD, filename: "周报.md")
        XCTAssertEqual(fallbackTitle, "周报")

        // 5. Atomic save update
        let updatedMD = """
        ---
        id: 01HZZZZZZZZZZZZZZZZZZZZZZZ
        tags:
          - Swift
          - Architecture
          - macOS
        created: 2026-08-23T10:00:00+08:00
        updated: 2026-08-23T10:30:00+08:00
        ---
        # Mieli 架构与状态机设计

        修改后的正文内容，增加了 macOS 原生 UI 集成说明。
        """
        try FileService.saveMarkdown(updatedMD, to: fileURL)
        let reloaded = try String(contentsOf: fileURL, encoding: .utf8)
        XCTAssertEqual(reloaded, updatedMD)
    }

    // MARK: - E2E Scenario 3: Folder Operations, Tree Navigation, and File Moving

    @MainActor
    func testFolderAndTreeNavigationOperations() throws {
        // Setup initial workspace folder hierarchy:
        // root/
        //   ├── 工作/
        //   │   └── 需求.md
        //   └── 学习/
        //       └── Swift.md
        let workDir = workspaceURL.appendingPathComponent("工作", isDirectory: true)
        let studyDir = workspaceURL.appendingPathComponent("学习", isDirectory: true)
        try FileManager.default.createDirectory(at: workDir, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: studyDir, withIntermediateDirectories: true)

        let reqURL = workDir.appendingPathComponent("需求.md")
        let swiftURL = studyDir.appendingPathComponent("Swift.md")
        try "# 需求分析\n内容".write(to: reqURL, atomically: true, encoding: .utf8)
        try "# Swift 基础\n内容".write(to: swiftURL, atomically: true, encoding: .utf8)

        // 1. Scan workspace tree
        var items = try WorkspaceScanner.scan(workspaceURL)
        XCTAssertEqual(items.count, 2) // "工作", "学习"
        let folderNames = Set(items.map(\.name))
        XCTAssertTrue(folderNames.contains("工作"))
        XCTAssertTrue(folderNames.contains("学习"))

        // 2. Create new subfolder
        let subDir = workDir.appendingPathComponent("项目A", isDirectory: true)
        try FileManager.default.createDirectory(at: subDir, withIntermediateDirectories: true)

        // 3. Move Swift.md from "学习" to "工作/项目A/Swift.md"
        let targetSwiftURL = subDir.appendingPathComponent("Swift.md")
        try FileManager.default.moveItem(at: swiftURL, to: targetSwiftURL)
        XCTAssertFalse(FileManager.default.fileExists(atPath: swiftURL.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: targetSwiftURL.path))

        // 4. Rename folder "学习" -> "归档"
        let archiveDir = workspaceURL.appendingPathComponent("归档", isDirectory: true)
        try FileManager.default.moveItem(at: studyDir, to: archiveDir)
        XCTAssertFalse(FileManager.default.fileExists(atPath: studyDir.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: archiveDir.path))

        // 5. Re-scan tree to verify structure
        items = try WorkspaceScanner.scan(workspaceURL)
        let updatedNames = Set(items.map(\.name))
        XCTAssertTrue(updatedNames.contains("工作"))
        XCTAssertTrue(updatedNames.contains("归档"))
    }

    // MARK: - E2E Scenario 4: Attachments Pipeline & Note Rename Link Rewriting

    @MainActor
    func testAttachmentsPipelineAndRenameRewriting() throws {
        let noteRelPath = "方案.md"
        let noteURL = workspaceURL.appendingPathComponent(noteRelPath)

        // 1. Create a dummy attachment file
        let sourceImageURL = workspaceURL.appendingPathComponent("temp_image.png")
        try Data([0x89, 0x50, 0x4E, 0x47]).write(to: sourceImageURL)

        // 2. Copy attachment for the note
        let copiedDestURL1 = try AttachmentService.copyAttachment(from: sourceImageURL,
                                                                 forNoteRelativePath: noteRelPath,
                                                                 in: workspaceURL)
        XCTAssertEqual(copiedDestURL1.lastPathComponent, "temp_image.png")

        let filesDir = workspaceURL.appendingPathComponent("方案.files", isDirectory: true)
        XCTAssertTrue(FileManager.default.fileExists(atPath: filesDir.path))
        let copiedFile1 = filesDir.appendingPathComponent("temp_image.png")
        XCTAssertTrue(FileManager.default.fileExists(atPath: copiedFile1.path))

        // 3. Handle filename collision via availableName / copyAttachmentUnique
        let copiedDestURL2 = try AttachmentService.copyAttachmentUnique(from: sourceImageURL,
                                                                       forNoteRelativePath: noteRelPath,
                                                                       in: workspaceURL)
        XCTAssertEqual(copiedDestURL2.lastPathComponent, "temp_image-1.png")
        let copiedFile2 = filesDir.appendingPathComponent("temp_image-1.png")
        XCTAssertTrue(FileManager.default.fileExists(atPath: copiedFile2.path))

        // 4. Create markdown referencing attachments
        let mdWithImages = """
        # 系统方案
        这里是架构图：
        ![架构图](./方案.files/temp_image.png)
        ![副图](./方案.files/temp_image-1.png)
        """
        try FileService.saveMarkdown(mdWithImages, to: noteURL)

        // 5. Rename note "方案.md" -> "新方案.md" along with its attachments folder
        let newNoteURL = workspaceURL.appendingPathComponent("新方案.md")
        try FileManager.default.moveItem(at: noteURL, to: newNoteURL)

        let renameFolderSuccess = AttachmentService.renameAttachmentFolder(fromNoteStem: "方案",
                                                                          toNoteStem: "新方案",
                                                                          in: workspaceURL)
        XCTAssertTrue(renameFolderSuccess)

        // Rewrite relative links in markdown
        let currentMD = try String(contentsOf: newNoteURL, encoding: .utf8)
        let rewrittenMD = AttachmentService.rewritingAttachmentLinks(currentMD,
                                                                    oldStem: "方案",
                                                                    newStem: "新方案")
        try FileService.saveMarkdown(rewrittenMD, to: newNoteURL)

        // Verify attachment folder moved and content rewritten
        let newFilesDir = workspaceURL.appendingPathComponent("新方案.files", isDirectory: true)
        XCTAssertFalse(FileManager.default.fileExists(atPath: filesDir.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: newFilesDir.path))
        let finalMD = try String(contentsOf: newNoteURL, encoding: .utf8)
        XCTAssertTrue(finalMD.contains("./新方案.files/temp_image.png"))
        XCTAssertTrue(finalMD.contains("./新方案.files/temp_image-1.png"))
        XCTAssertFalse(finalMD.contains("./方案.files/"))
    }

    // MARK: - E2E Scenario 5: Full-Text Search (FTS5), Chinese/English, Ranking & Tags

    @MainActor
    func testFullTextSearchAndTagsEndToEnd() throws {
        let dbQueue = try DatabaseManager.openDatabaseQueue(at: workspaceURL)

        // 1. Seed several notes with Chinese and English content
        let note1 = """
        ---
        id: 01HNOTE1111111111111111111
        tags:
          - Swift
          - 状态机
        created: 2026-08-23T10:00:00+08:00
        updated: 2026-08-23T10:00:00+08:00
        ---
        # 状态机实现方案

        今天详细研究了 Spring 状态机和 Swift 状态机的实现方案，性能表现优异。
        """

        let note2 = """
        ---
        id: 01HNOTE2222222222222222222
        tags:
          - Swift
          - Concurrency
        created: 2026-08-23T11:00:00+08:00
        updated: 2026-08-23T11:00:00+08:00
        ---
        # Swift 并发编程模型

        使用 async/await 和 Actor 构建线程安全的应用架构。状态机可以配合 actor 使用。
        """

        let note3 = """
        ---
        id: 01HNOTE3333333333333333333
        tags:
          - Daily
        created: 2026-08-23T12:00:00+08:00
        updated: 2026-08-23T12:00:00+08:00
        ---
        # 日常随笔

        今天天气晴朗，散步放松心情。
        """

        try IndexService.index(ParsedNote(relativePath: "技术/状态机.md", markdown: note1), in: dbQueue)
        try IndexService.index(ParsedNote(relativePath: "技术/Swift并发.md", markdown: note2), in: dbQueue)
        try IndexService.index(ParsedNote(relativePath: "生活/日记.md", markdown: note3), in: dbQueue)

        // 2. Search Chinese tokens
        let hitsZhuangTaiJi = try SearchService.search("状态机", in: dbQueue)
        XCTAssertEqual(hitsZhuangTaiJi.count, 2)
        // note1 has "状态机" in Title + Tags + Content, should rank first
        XCTAssertEqual(hitsZhuangTaiJi.first?.id, "01HNOTE1111111111111111111")
        XCTAssertEqual(hitsZhuangTaiJi.first?.title, "状态机实现方案")

        let hitsShiXianFangAn = try SearchService.search("实现方案", in: dbQueue)
        XCTAssertEqual(hitsShiXianFangAn.count, 1)
        XCTAssertEqual(hitsShiXianFangAn.first?.id, "01HNOTE1111111111111111111")

        // 3. Search English token
        let hitsSwift = try SearchService.search("Swift", in: dbQueue)
        XCTAssertEqual(hitsSwift.count, 2)

        // 4. Tags query & filtering
        let allTags = try dbQueue.read { db in
            try String.fetchAll(db, sql: "SELECT name FROM tags ORDER BY name ASC")
        }
        XCTAssertTrue(allTags.contains("Swift"))
        XCTAssertTrue(allTags.contains("状态机"))
        XCTAssertTrue(allTags.contains("Concurrency"))
        XCTAssertTrue(allTags.contains("Daily"))

        // 5. Test index rebuild without modifying markdown files
        let files = [
            "技术/状态机.md": note1,
            "技术/Swift并发.md": note2,
            "生活/日记.md": note3
        ]
        for (rel, content) in files {
            let u = workspaceURL.appendingPathComponent(rel)
            try FileManager.default.createDirectory(at: u.deletingLastPathComponent(), withIntermediateDirectories: true)
            try content.write(to: u, atomically: true, encoding: .utf8)
        }

        try IndexRebuilder.rebuild(workspaceURL: workspaceURL, in: dbQueue)

        let recheckHits = try SearchService.search("状态机", in: dbQueue)
        XCTAssertEqual(recheckHits.count, 2)

        try dbQueue.close()
    }

    // MARK: - E2E Scenario 6: Watcher, External Mod, and Self-Write Suppression

    @MainActor
    func testWatcherReconciliationAndSelfWriteSuppression() async throws {
        let noteRelPath = "watch_test.md"
        let noteURL = workspaceURL.appendingPathComponent(noteRelPath)
        try "# 观察测试\n初始内容".write(to: noteURL, atomically: true, encoding: .utf8)

        let bookmarkData = try XCTUnwrap(workspaceManager.createBookmark(for: workspaceURL))
        _ = workspaceManager.persist(bookmarkData, to: bookmarkFileURL)
        _ = workspaceManager.restoreWorkspace(bookmarkSource: bookmarkFileURL)
        workspaceManager.openedNotePath = noteRelPath

        let initialEventID = workspaceManager.externalEventID

        // 1. App's own write with `recordSelfWrite` -> no conflict trigger
        try FileService.saveMarkdown("# 观察测试\n应用内保存内容", to: noteURL)
        workspaceManager.recordSelfWrite(at: noteRelPath)

        try await Task.sleep(for: .seconds(2.5))
        XCTAssertEqual(workspaceManager.externalEventID, initialEventID,
                       "App's own autosave must not trigger conflict")

        // 2. External program edit -> conflict triggered
        try "# 观察测试\n外部编辑器写入的内容".write(to: noteURL, atomically: true, encoding: .utf8)

        try await Task.sleep(for: .seconds(2.5))
        XCTAssertGreaterThan(workspaceManager.externalEventID, initialEventID,
                             "External modification must be detected as conflict")
    }

    // MARK: - E2E Scenario 7: All Markdown Notes Tag Rendering & Lifecycle Verification

    @MainActor
    func testAllMarkdownNotesTagRenderingAndLifecycle() async throws {
        // 1. Initialize workspace with diverse markdown files
        let bookmarkData = try XCTUnwrap(workspaceManager.createBookmark(for: workspaceURL))
        _ = workspaceManager.persist(bookmarkData, to: bookmarkFileURL)
        _ = workspaceManager.restoreWorkspace(bookmarkSource: bookmarkFileURL)

        let notesMap: [String: String] = [
            // Standard YAML block tags with Chinese & English
            "架构/系统设计.md": """
            ---
            id: 01HNOTE001
            tags:
              - Swift
              - macOS
              - 架构设计
            created: 2026-08-23T10:00:00+08:00
            updated: 2026-08-23T10:00:00+08:00
            ---
            # 系统设计
            正文内容。
            - 这里的列表项不是标签
            """,

            // Inline array tags
            "前端/编辑器.md": """
            ---
            id: 01HNOTE002
            tags: [前端, UI, Editor]
            created: 2026-08-23T11:00:00+08:00
            updated: 2026-08-23T11:00:00+08:00
            ---
            # 编辑器开发
            ProseMirror 方案说明。
            """,

            // Empty tags array
            "工作/待办.md": """
            ---
            id: 01HNOTE003
            tags: []
            created: 2026-08-23T12:00:00+08:00
            updated: 2026-08-23T12:00:00+08:00
            ---
            # 待办事项
            无标签笔记。
            """,

            // Front matter without tags key
            "工作/周报.md": """
            ---
            id: 01HNOTE004
            created: 2026-08-23T13:00:00+08:00
            updated: 2026-08-23T13:00:00+08:00
            ---
            # 本周总结
            工作进展汇报。
            """,

            // Plain note without Front Matter
            "生活/随笔.md": """
            # 随笔
            完全没有 Front Matter 的笔记。
            - 随笔条目 1
            - 随笔条目 2
            """,

            // Nested folder note with overlapping tag "Swift"
            "学习/Swift进阶/并发.md": """
            ---
            id: 01HNOTE005
            tags:
              - Swift
              - 并发编程
            created: 2026-08-23T14:00:00+08:00
            updated: 2026-08-23T14:00:00+08:00
            ---
            # Swift 并发编程
            Actor 与 Task 深入探讨。
            """
        ]

        for (relPath, content) in notesMap {
            let fileURL = workspaceURL.appendingPathComponent(relPath)
            try FileManager.default.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try content.write(to: fileURL, atomically: true, encoding: .utf8)
        }

        // Rebuild index to index all files into SQLite
        guard let dbQueue = workspaceManager.databaseQueue else {
            XCTFail("Database queue must not be nil")
            return
        }
        try IndexRebuilder.rebuild(workspaceURL: workspaceURL, in: dbQueue)

        // 2. Read each source .md file from disk and assert its tag rendering data matches exactly
        for (relPath, originalContent) in notesMap {
            let fileURL = workspaceURL.appendingPathComponent(relPath)
            XCTAssertTrue(FileManager.default.fileExists(atPath: fileURL.path))

            // Read the source .md file directly from disk
            let sourceMD = try String(contentsOf: fileURL, encoding: .utf8)
            XCTAssertEqual(sourceMD, originalContent)

            // Extract tags via domain parser
            let expectedTags = NoteTags.tags(in: sourceMD)

            // Query tags via WorkspaceManager (used for note tag chips rendering in UI)
            let actualTags = workspaceManager.tags(forNoteAt: relPath)
            XCTAssertEqual(actualTags, expectedTags, "Tags for \(relPath) must match source .md Front Matter tags")

            // Specific assertions for different note types
            if relPath == "架构/系统设计.md" {
                XCTAssertEqual(actualTags, ["Swift", "macOS", "架构设计"])
            } else if relPath == "前端/编辑器.md" {
                XCTAssertEqual(actualTags, ["前端", "UI", "Editor"])
            } else if relPath == "工作/待办.md" {
                XCTAssertEqual(actualTags, [], "Empty tags array in FM must render as empty tags list (UI displays '无标签')")
            } else if relPath == "工作/周报.md" {
                XCTAssertEqual(actualTags, [], "Missing tags key in FM must render as empty tags list (UI displays '无标签')")
            } else if relPath == "生活/随笔.md" {
                XCTAssertEqual(actualTags, [], "No FM note must render as empty tags list (UI displays '无标签')")
            } else if relPath == "学习/Swift进阶/并发.md" {
                XCTAssertEqual(actualTags, ["Swift", "并发编程"])
            }
        }

        // 3. Verify workspace-wide aggregated tags (Sidebar tags list)
        let sidebarTags = workspaceManager.allTags()
        let expectedAllDistinctTags = ["Editor", "macOS", "Swift", "UI", "前端", "并发编程", "架构设计"]
        XCTAssertEqual(sidebarTags.sorted(), expectedAllDistinctTags.sorted())

        // 4. Verify tag filtering across all notes
        // Filter by "Swift" -> should return 2 notes ("架构/系统设计.md" and "学习/Swift进阶/并发.md")
        let swiftNotes = workspaceManager.notes(withTag: "Swift")
        XCTAssertEqual(swiftNotes.count, 2)
        let swiftNotePaths = Set(swiftNotes.map(\.relativePath))
        XCTAssertEqual(swiftNotePaths, ["架构/系统设计.md", "学习/Swift进阶/并发.md"])

        // Filter by "架构设计" -> should return 1 note
        let archNotes = workspaceManager.notes(withTag: "架构设计")
        XCTAssertEqual(archNotes.count, 1)
        XCTAssertEqual(archNotes.first?.relativePath, "架构/系统设计.md")
        XCTAssertEqual(archNotes.first?.title, "系统设计")

        // 5. Test adding a tag to a note with NO front matter ("生活/随笔.md")
        let addSuccess = workspaceManager.addTag("新标签", toNoteAt: "生活/随笔.md")
        XCTAssertTrue(addSuccess)

        // Allow detached index update to complete
        try await Task.sleep(for: .milliseconds(500))

        // Read source .md from disk to verify atomic modification of Front Matter
        let modifiedSuiBi = try String(contentsOf: workspaceURL.appendingPathComponent("生活/随笔.md"), encoding: .utf8)
        XCTAssertTrue(modifiedSuiBi.contains("tags:\n  - 新标签\n"))
        XCTAssertTrue(modifiedSuiBi.contains("# 随笔"))
        XCTAssertTrue(modifiedSuiBi.contains("- 随笔条目 1"))

        // Verify tags query and sidebar aggregation after add
        let suibiTags = workspaceManager.tags(forNoteAt: "生活/随笔.md")
        XCTAssertEqual(suibiTags, ["新标签"])
        XCTAssertTrue(workspaceManager.allTags().contains("新标签"))
        let taggedSuiBiNotes = workspaceManager.notes(withTag: "新标签")
        XCTAssertEqual(taggedSuiBiNotes.map(\.relativePath), ["生活/随笔.md"])

        // 6. Test removing a tag from a note with multiple tags ("架构/系统设计.md")
        let removeSuccess = workspaceManager.removeTag("macOS", fromNoteAt: "架构/系统设计.md")
        XCTAssertTrue(removeSuccess)

        // Allow detached index update to complete
        try await Task.sleep(for: .milliseconds(500))

        // Read source .md from disk to verify atomic tag removal
        let modifiedArch = try String(contentsOf: workspaceURL.appendingPathComponent("架构/系统设计.md"), encoding: .utf8)
        XCTAssertFalse(modifiedArch.contains("macOS"))
        XCTAssertTrue(modifiedArch.contains("Swift"))
        XCTAssertTrue(modifiedArch.contains("架构设计"))
        XCTAssertTrue(modifiedArch.contains("- 这里的列表项不是标签"))

        let updatedArchTags = workspaceManager.tags(forNoteAt: "架构/系统设计.md")
        XCTAssertEqual(updatedArchTags, ["Swift", "架构设计"])

        // Since no other note had "macOS", "macOS" must be removed from sidebarTags
        XCTAssertFalse(workspaceManager.allTags().contains("macOS"))
        XCTAssertEqual(workspaceManager.notes(withTag: "macOS"), [])
    }

    // MARK: - E2E Scenario 8: Save, Tag, and Rename Preserve Editor Content

    @MainActor
    func testSavedEditorContentSurvivesTagAndRenameMutations() throws {
        let bookmarkData = try XCTUnwrap(workspaceManager.createBookmark(for: workspaceURL))
        XCTAssertTrue(workspaceManager.persist(bookmarkData, to: bookmarkFileURL))
        _ = workspaceManager.restoreWorkspace(bookmarkSource: bookmarkFileURL)

        let originalPath = "编辑中的笔记.md"
        let originalURL = workspaceURL.appendingPathComponent(originalPath)
        let editedMarkdown = """
        ---
        id: 01HT115EDITORCONTENT000001
        tags: []
        created: 2026-08-23T14:00:00+08:00
        updated: 2026-08-23T14:00:00+08:00
        ---
        # 尚未自动保存的标题

        用户刚刚输入的正文必须保留。
        """

        // Mirrors T115's required ordering: persist the latest editor value,
        // then mutate Front Matter, then rename the file-backed note.
        try FileService.saveMarkdown(editedMarkdown, to: originalURL)
        XCTAssertTrue(workspaceManager.addTag("T115", toNoteAt: originalPath))
        XCTAssertTrue(workspaceManager.renameNote(at: originalPath, to: "已重命名笔记"))

        let renamedURL = workspaceURL.appendingPathComponent("已重命名笔记.md")
        XCTAssertFalse(FileManager.default.fileExists(atPath: originalURL.path))
        let reloaded = try String(contentsOf: renamedURL, encoding: .utf8)
        XCTAssertTrue(reloaded.contains("  - T115"))
        XCTAssertTrue(reloaded.contains("# 尚未自动保存的标题"))
        XCTAssertTrue(reloaded.contains("用户刚刚输入的正文必须保留。"))
    }
}
