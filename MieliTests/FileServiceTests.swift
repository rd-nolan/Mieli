import XCTest
@testable import Mieli

/// Verifies T029: safe, atomic Markdown persistence.
final class FileServiceTests: XCTestCase {

    private var dir: URL!

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mieli-fservice-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: dir)
        dir = nil
    }

    private func noteURL(_ name: String) -> URL {
        dir.appendingPathComponent(name)
    }

    private func tmpExists(for url: URL) -> Bool {
        let tmp = url.deletingLastPathComponent()
            .appendingPathComponent(".\(url.lastPathComponent).tmp")
        return FileManager.default.fileExists(atPath: tmp.path)
    }

    func testSavesNewFile() throws {
        let url = noteURL("新笔记.md")
        XCTAssertTrue(try FileService.saveMarkdown("# 新标题\n内容行\n", to: url))
        let content = try String(contentsOf: url, encoding: .utf8)
        XCTAssertEqual(content, "# 新标题\n内容行\n")
        XCTAssertFalse(tmpExists(for: url))
    }

    func testOverwritesExistingFile() throws {
        let url = noteURL("笔记.md")
        try "旧内容\n".write(to: url, atomically: true, encoding: .utf8)
        XCTAssertTrue(try FileService.saveMarkdown("# 新\n正文", to: url))
        let content = try String(contentsOf: url, encoding: .utf8)
        XCTAssertEqual(content, "# 新\n正文")
        XCTAssertFalse(tmpExists(for: url))
    }

    func testPreservesChineseAndMultilineContent() throws {
        let url = noteURL("中文.md")
        let text = """
        # 今天研究了 Spring 状态机
        第 2 行：实现方案。
        第 3 行：Swift + macOS。
        """
        XCTAssertTrue(try FileService.saveMarkdown(text, to: url))
        let content = try String(contentsOf: url, encoding: .utf8)
        XCTAssertEqual(content, text)
        XCTAssertFalse(tmpExists(for: url))
    }

    func testLeavesNoTemporaryResidueOnSuccess() throws {
        let url = noteURL("干净.md")
        XCTAssertTrue(try FileService.saveMarkdown("ok", to: url))
        let leftovers = try FileManager.default.contentsOfDirectory(atPath: dir.path)
        XCTAssertEqual(leftovers, ["干净.md"])
    }

    func testMissingDirectoryFails() {
        let missingDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("not-a-dir-\(UUID().uuidString)")
        let url = missingDir.appendingPathComponent("x.md")
        XCTAssertThrowsError(
            try FileService.saveMarkdown("内容", to: url)
        ) { error in
            XCTAssertEqual(error as? FileService.SaveFailure,
                           .missingTarget)
        }
    }

    func testFailureLeavesExistingFileIntact() throws {
        // Save to a read-only directory so the temp write fails, then confirm
        // the existing file was not modified.
        let roDir = dir.appendingPathComponent("ro", isDirectory: true)
        try FileManager.default.createDirectory(at: roDir, withIntermediateDirectories: true)
        let target = roDir.appendingPathComponent("笔记.md")
        try "原内容\n".write(to: target, atomically: true, encoding: .utf8)
        // Make the directory non-writable after creating the file.
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o555], ofItemAtPath: roDir.path)
        defer {
            try? FileManager.default.setAttributes(
                [.posixPermissions: 0o755], ofItemAtPath: roDir.path)
        }

        XCTAssertThrowsError(
            try FileService.saveMarkdown("# 新", to: target)
        ) { error in
            XCTAssertEqual(error as? FileService.SaveFailure,
                           .tempWriteFailed)
        }
        // Original content unchanged.
        let content = try String(contentsOf: target, encoding: .utf8)
        XCTAssertEqual(content, "原内容\n")
        XCTAssertFalse(tmpExists(for: target))
    }

    // MARK: - T080 attachment directory

    private func makeNote(_ rel: String, at root: URL) throws -> URL {
        let url = root.appendingPathComponent(rel)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data("# 笔记\n".utf8).write(to: url)
        return url
    }

    func testCreatesAttachmentDirectoryBesideNote() throws {
        try makeNote("工作/技术方案.md", at: dir)
        let attDir = try AttachmentService.attachmentDirectory(
            forNoteRelativePath: "工作/技术方案.md", in: dir)
        XCTAssertEqual(attDir.lastPathComponent, "技术方案.files")
        XCTAssertEqual(attDir.deletingLastPathComponent().lastPathComponent, "工作")
        XCTAssertTrue(FileManager.default.fileExists(
            atPath: attDir.path, isDirectory: nil),
            "attachment folder must exist on disk")
    }

    func testAttachmentDirectoryIsIdempotent() throws {
        try makeNote("笔记.md", at: dir)
        let first = try AttachmentService.attachmentDirectory(
            forNoteRelativePath: "笔记.md", in: dir)
        let second = try AttachmentService.attachmentDirectory(
            forNoteRelativePath: "笔记.md", in: dir)
        XCTAssertEqual(first.path, second.path)
        XCTAssertTrue(FileManager.default.fileExists(atPath: first.path))
    }

    func testAttachmentDirectoryForSubfolderNote() throws {
        try makeNote("私密/周记.md", at: dir)
        let attDir = try AttachmentService.attachmentDirectory(
            forNoteRelativePath: "私密/周记.md", in: dir)
        XCTAssertEqual(attDir.lastPathComponent, "周记.files")
    }

    func testAttachmentDirectoryThrowsWhenNoteParentMissing() {
        XCTAssertThrowsError(
            try AttachmentService.attachmentDirectory(
                forNoteRelativePath: "不存在的文件夹/笔记.md", in: dir)
        ) { error in
            XCTAssertEqual(error as? FileService.SaveFailure, .missingTarget)
        }
    }

    // MARK: - T081 copy attachment

    private func makeSource(named name: String) throws -> URL {
        let url = dir.appendingPathComponent(name)
        try Data("бинокль-\(name)".utf8).write(to: url)
        return url
    }

    func testCopiesAttachmentIntoNoteFolder() throws {
        try makeNote("工作/笔记.md", at: dir)
        let source = try makeSource(named: "photo.png")
        let dest = try AttachmentService.copyAttachment(
            from: source, forNoteRelativePath: "工作/笔记.md", in: dir)
        XCTAssertEqual(dest.lastPathComponent, "photo.png")
        XCTAssertEqual(dest.deletingLastPathComponent().lastPathComponent, "笔记.files")
        let copied = try String(contentsOf: dest, encoding: .utf8)
        XCTAssertEqual(copied, "бинокль-photo.png")
    }

    func testCopyRefusesOverwriteExisting() throws {
        try makeNote("笔记.md", at: dir)
        let source = try makeSource(named: "a.png")
        _ = try AttachmentService.copyAttachment(
            from: source, forNoteRelativePath: "笔记.md", in: dir)
        // Dragging the same-named file again must not overwrite the copy.
        XCTAssertThrowsError(
            try AttachmentService.copyAttachment(
                from: source, forNoteRelativePath: "笔记.md", in: dir)
        ) { error in
            XCTAssertEqual(error as? AttachmentService.CopyError, .fileExists)
        }
        // The original copied content is untouched.
        let folder = try AttachmentService.attachmentDirectory(
            forNoteRelativePath: "笔记.md", in: dir)
        let kept = try String(contentsOf: folder.appendingPathComponent("a.png"), encoding: .utf8)
        XCTAssertEqual(kept, "бинокль-a.png")
    }

    func testCopyAttachmentForSubfolderNote() throws {
        try makeNote("私密/周记.md", at: dir)
        let source = try makeSource(named: "截图.png")
        let dest = try AttachmentService.copyAttachment(
            from: source, forNoteRelativePath: "私密/周记.md", in: dir)
        XCTAssertEqual(dest.deletingLastPathComponent().lastPathComponent, "周记.files")
    }

    // MARK: - T082 unique filename

    func testAvailableNamePrefersFreeName() throws {
        let folder = try AttachmentService.attachmentDirectory(
            forNoteRelativePath: "笔记.md", in: dir)
        XCTAssertEqual(
            AttachmentService.availableName(preferredName: "a.png", in: folder), "a.png")
    }

    func testAvailableNameIncrementsUntilFree() throws {
        let folder = try AttachmentService.attachmentDirectory(
            forNoteRelativePath: "笔记.md", in: dir)
        try Data("1".utf8).write(to: folder.appendingPathComponent("a.png"))
        XCTAssertEqual(
            AttachmentService.availableName(preferredName: "a.png", in: folder), "a-1.png")
        try Data("2".utf8).write(to: folder.appendingPathComponent("a-1.png"))
        XCTAssertEqual(
            AttachmentService.availableName(preferredName: "a.png", in: folder), "a-2.png")
    }

    func testAvailableNameHandlesExtensionlessFile() throws {
        let folder = try AttachmentService.attachmentDirectory(
            forNoteRelativePath: "笔记.md", in: dir)
        try Data("x".utf8).write(to: folder.appendingPathComponent("LICENSE"))
        XCTAssertEqual(
            AttachmentService.availableName(preferredName: "LICENSE", in: folder), "LICENSE-1")
    }

    func testCopyAttachmentUniqueNeverOverwrites() throws {
        try makeNote("笔记.md", at: dir)
        let source = try makeSource(named: "shot.png")
        let first = try AttachmentService.copyAttachmentUnique(
            from: source, forNoteRelativePath: "笔记.md", in: dir)
        XCTAssertEqual(first.lastPathComponent, "shot.png")
        // Dragging the same name again must produce shot-1.png, not overwrite.
        let second = try AttachmentService.copyAttachmentUnique(
            from: source, forNoteRelativePath: "笔记.md", in: dir)
        XCTAssertEqual(second.lastPathComponent, "shot-1.png")
        // Both files exist.
        let folder = first.deletingLastPathComponent()
        XCTAssertTrue(FileManager.default.fileExists(atPath: folder.appendingPathComponent("shot.png").path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: folder.appendingPathComponent("shot-1.png").path))
    }
}