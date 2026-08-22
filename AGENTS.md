Minne — Development Guide

你正在开发一个名为 Minne 的 macOS 原生 Markdown 笔记应用。

本文件是项目的长期开发规范、架构约束和任务 Roadmap。

在进行任何代码修改之前，必须完整阅读本文件。

⸻

1. Product Goal

Minne 是一个：

简洁、快速、Local-first 的 macOS Markdown 笔记软件。

核心能力只有：

* 本地 Workspace
* 文件夹
* Markdown 笔记
* Markdown 所见即所得编辑
* 标签
* 附件
* 全文搜索

Minne 不应该发展成：

* Notion Clone
* Obsidian Clone
* 云笔记平台
* 协作平台
* AI 平台
* 项目管理工具

保持产品简单。

⸻

2. Core Principle

Minne 最重要的数据原则：

Markdown + Attachments = User Data
SQLite = Rebuildable Local Index

Markdown 文件和附件是真正的数据。

SQLite 只是：

* metadata index
* search index
* performance cache

删除：

.minne/index.sqlite

之后必须能够仅根据 Workspace 中的 Markdown 文件重新建立索引。

⸻

3. Platform

当前只开发：

macOS

技术栈：

* Swift
* SwiftUI
* 必要时使用 AppKit
* WKWebView
* Markdown-first WYSIWYG Editor
* GRDB.swift
* SQLite
* SQLite FTS5
* Swift Concurrency
* OSLog / Logger

未来可能考虑：

* iPhone
* iPad

但当前禁止因为未来平台需求增加额外复杂度。

不支持：

* Windows
* Linux
* Web

⸻

4. Engineering Philosophy

始终遵循：

KISS

Keep It Simple.

如果两个方案都能满足当前需求：

优先选择：

* 更少代码
* 更少依赖
* 更少抽象
* 更容易理解
* 更容易测试
* 更容易维护

的方案。

⸻

YAGNI

You Aren’t Gonna Need It.

不要因为：

以后可能需要

提前开发功能。

禁止提前设计：

* Sync architecture
* Plugin architecture
* AI architecture
* Collaboration architecture
* CRDT
* EventBus
* CQRS
* Remote storage abstraction
* Device abstraction
* Complex Repository Layer
* Complex DI Framework

只有当前任务真正需要时才增加抽象。

⸻

5. Data Safety Priority

Minne 管理的是用户笔记。

数据安全优先级最高。

开发决策优先级：

Data Safety
>
Correctness
>
Simplicity
>
Native macOS UX
>
Performance
>
Architecture Elegance
>
Future Possibility

任何情况下：

Never silently lose user data.

⸻

6. Workspace

用户第一次启动 Minne 时选择一个本地目录作为 Workspace。

例如：

~/Documents/Notes

Workspace 中直接保存用户数据。

例如：

Notes/
├── 工作/
│   ├── 项目A/
│   │   ├── 需求分析.md
│   │   ├── 需求分析.files/
│   │   │   └── architecture.png
│   │   └── 技术方案.md
│   │
│   └── 周报.md
│
├── 学习/
│   ├── Swift.md
│   └── SQLite.md
│
└── .minne/
    └── index.sqlite

.minne 是 Minne 内部目录。

扫描 Workspace 时必须忽略：

.minne

⸻

7. Workspace Access

考虑 macOS Sandbox。

Workspace 由用户通过系统目录选择器选择。

使用：

Security-Scoped Bookmark

保存访问权限。

需要支持：

* choose workspace
* create bookmark
* persist bookmark
* restore bookmark
* stale bookmark handling
* startAccessingSecurityScopedResource
* stopAccessingSecurityScopedResource

这些逻辑必须集中在 Workspace 模块。

不要散落在 SwiftUI Views 中。

⸻

8. Markdown Is Source of Truth

每篇笔记都是普通：

*.md

文件。

禁止把：

* SQLite
* HTML
* ProseMirror JSON
* Editor JSON
* NSAttributedString archive

作为笔记唯一主数据。

编辑器最终必须生成 Markdown。

⸻

9. Markdown Metadata

Minne 创建的 Markdown 使用 YAML Front Matter。

例如：

---
id: 01K32M4PZXXXXXXXX
tags:
  - Swift
  - macOS
created: 2026-08-21T08:00:00+08:00
updated: 2026-08-21T08:30:00+08:00
---
# Swift Concurrency
这里是正文。

V1 metadata 只有：

id
tags
created
updated

不要自行增加：

favorites
sync
revision
device
cloud
AI
embedding

等 metadata。

⸻

10. Stable Note ID

每篇笔记拥有稳定 ID。

优先：

ULID

例如：

01K32M4PZXXXXXXXX

即使：

工作/技术方案.md

移动成：

归档/2026/系统设计.md

ID 也不能改变。

禁止使用：

* filename
* absolute path
* relative path

作为永久 Note ID。

⸻

11. Note Title

标题规则：

优先：

Markdown 第一个 H1

如果不存在 H1：

filename without extension

例如：

hello.md

内容：

# Swift Concurrency

title：

Swift Concurrency

如果没有 H1：

hello

⸻

12. Folder Model

Minne 不维护虚拟目录。

Sidebar 中的文件夹：

就是操作系统中的真实目录。

例如：

工作
  项目A
    技术方案

对应：

工作/
工作/项目A/
工作/项目A/技术方案.md

不要在 SQLite 中重新维护一套 Folder hierarchy。

⸻

13. File Operations

V1 支持：

* scan workspace
* create folder
* rename folder
* delete folder
* create Markdown
* rename Markdown
* move Markdown
* delete Markdown
* read Markdown
* save Markdown

所有操作必须真正作用于文件系统。

⸻

14. Safe Save

Markdown 属于用户重要数据。

保存时优先使用安全 atomic write。

原则：

note.md.tmp
      ↓
write complete content
      ↓
success
      ↓
atomic replace
      ↓
note.md

需要考虑：

* write failure
* permission error
* file removed externally
* disk full
* app crash

不要因为保存失败导致原文件变成空文件。

⸻

15. Markdown Editor

Minne 需要：

WYSIWYG Markdown Editor

用户看到的是接近排版后的内容。

例如 Markdown：

# Hello
**World**

编辑器应该显示：

Hello
World

而不是始终显示 Markdown syntax。

优先考虑：

WKWebView
+
Milkdown / ProseMirror

但不要盲目锁死技术方案。

如果实际实现过程中确认存在：

* 更简单
* 更稳定
* Markdown-first
* macOS WKWebView 集成更好

的方案，可以提出理由后调整。

⸻

16. Editor Boundary

编辑器必须有明确 Bridge。

推荐：

SwiftUI
   ↓
MarkdownEditorView
   ↓
EditorWebView
   ↓
EditorBridge
   ↓
WKWebView
   ↓
Markdown Editor

Swift → Editor：

loadMarkdown
focus
setEditable
insertAttachment

Editor → Swift：

editorReady
contentChanged
attachmentDropped
linkClicked

禁止让其他业务模块到处执行：

evaluateJavaScript(...)

所有 Editor communication 集中管理。

⸻

17. Auto Save

支持自动保存。

不要每次 keystroke 都写磁盘。

使用 debounce。

推荐初始值：

750ms

作为集中配置常量。

流程：

Editor Change
     ↓
Debounce
     ↓
NoteService
     ↓
FileService
     ↓
Atomic Save
     ↓
Index Update

⸻

18. Attachments

附件属于具体笔记。

例如：

技术方案.md

附件：

技术方案.files/

完整结构：

技术方案.md
技术方案.files/
├── architecture.png
├── api.pdf
└── example.zip

Markdown 使用 relative path：

![Architecture](./技术方案.files/architecture.png)
[API](./技术方案.files/api.pdf)

⸻

19. Attachment Drag & Drop

允许把普通文件拖入 Editor。

基本流程：

Drop File
    ↓
AttachmentService
    ↓
Create <note>.files if needed
    ↓
Copy file
    ↓
Return relative path
    ↓
Insert Markdown

支持至少：

* PNG
* JPG/JPEG
* GIF
* WebP
* PDF
* TXT
* ZIP
* 普通文件

⸻

20. Attachment Filename Conflict

如果：

image.png

已经存在：

禁止静默覆盖。

生成：

image-1.png
image-2.png
image-3.png

直到找到可用 filename。

⸻

21. Note Rename + Attachments

如果：

技术方案.md
技术方案.files/

改名：

系统设计.md

对应附件目录也应该改成：

系统设计.files/

并更新当前 Markdown 中指向该附件目录的 relative paths。

这一操作必须避免造成明显数据丢失。

⸻

22. SQLite

SQLite 位于：

<workspace>/.minne/index.sqlite

SQLite 只是：

Local Search Index

推荐：

GRDB.swift

不要使用 SwiftData 作为全文搜索核心。

⸻

23. SQLite Schema

V1 只创建真正需要的 schema。

推荐：

notes
tags
note_tags
note_fts

notes 至少包含：

id
relative_path
filename
title
folder
created_at
updated_at
file_mtime
file_size
content_hash

路径使用：

Workspace-relative path

不要把：

/Users/xxx/Documents/...

作为永久数据写入索引。

⸻

24. Forbidden Database Tables

禁止提前创建：

sync_state
sync_metadata
devices
remote_notes
revisions
history
backlinks
graph
embeddings
vectors
plugins
accounts
users
sessions

当前没有需求。

⸻

25. Search

全文搜索使用：

SQLite FTS5

索引：

title
filename
path
tags
content

用户输入一个关键词时：

同时搜索以上字段。

⸻

26. Chinese Search

Minne 必须重点支持：

* Chinese
* English
* Chinese + English

例如：

今天研究了 Spring 状态机的实现方案

搜索：

状态机

应该命中。

搜索：

实现方案

也应该命中。

V1 优先研究：

FTS5 trigram tokenizer

不要为了中文搜索引入 Elasticsearch、Meilisearch 等外部服务。

⸻

27. Search Ranking

结果优先级：

title
>
filename
>
tags
>
content
>
path

可以使用：

FTS5 bm25

实现。

不要开发复杂自定义 ranking engine。

⸻

28. Search Result

搜索结果至少包含：

Title
Folder / Path
matched snippet

如果实现简单，可以显示 tags。

不要为了 Search UI 提前开发复杂 preview system。

⸻

29. SQLite Index Update

注意：

Minne 中：

Markdown → SQLite

这个行为叫：

Index Update
Index Refresh
Index Rebuild
Index Reconciliation

禁止叫：

Sync
Synchronization

推荐类名：

IndexService
IndexUpdater
IndexRebuilder
FileChangeProcessor

禁止：

SyncService
SyncManager
SyncEngine

⸻

30. NO SYNC

这是项目最高优先级 Scope 约束之一。

Minne V1：

没有任何多设备同步。

禁止实现：

* CloudKit
* iCloud Sync
* custom cloud sync
* WebDAV
* Dropbox API
* Google Drive API
* Git Sync
* Syncthing integration
* remote storage
* device-to-device sync
* Mac ↔ Mac
* Mac ↔ iPhone
* Mac ↔ iPad
* Sync Engine
* Sync Protocol
* CRDT
* remote revision
* remote conflict resolution

甚至禁止：

为未来同步预留代码。

不要创建：

SyncService
CloudService
RemoteStore
DeviceManager
SyncMetadata

⸻

31. External File Changes

用户可能使用：

* Finder
* VS Code
* Typora
* 其他本地软件

修改 Markdown。

Minne 后期需要监听 Workspace。

如果用户自己把 Workspace 放进：

* iCloud Drive
* Dropbox
* Syncthing

这是用户自己的行为。

Minne 不集成这些服务。

Minne 只看到：

Local Filesystem Changes

⸻

32. File Watcher

File Watcher 后期实现。

负责：

create
modify
delete
rename
move

然后：

File Change
    ↓
FileChangeProcessor
    ↓
Index Update

不要把 FileWatcher 开发成 Sync Engine。

⸻

33. Startup Index Check

打开 Workspace 后：

扫描 Markdown。

SQLite 保存：

relative_path
mtime
size
hash

比较：

new
→ index
modified
→ reindex
deleted
→ remove index
unchanged
→ skip

优先比较：

mtime + size

必要时再 hash。

不要每次启动都无条件读取并重新索引所有 Markdown。

⸻

34. Rebuild Index

提供内部能力：

Rebuild Index

过程：

clear/recreate index
      ↓
scan *.md
      ↓
parse
      ↓
index

绝对不能修改用户 Markdown。

⸻

35. SwiftUI Architecture

不要过度架构。

推荐大致：

Minne/
├── App/
├── Workspace/
├── FileSystem/
├── Notes/
├── Search/
├── Editor/
├── Views/
└── Resources/

根据实际需要创建。

不要为了目录漂亮创建大量只有一个文件的模块。

⸻

36. Views

SwiftUI View 负责：

Presentation
Interaction
UI State

禁止直接负责：

SQLite query
Filesystem scan
Markdown parsing
Hash calculation
Index rebuild

⸻

37. Services

根据实际需求创建简单 Service。

例如：

WorkspaceManager
FileService
NoteService
IndexService
SearchService
AttachmentService

不要每个 Service 再套：

Protocol
Repository
UseCase
DataSource
Coordinator
Manager
Factory

除非确实存在实际需求。

⸻

38. Protocol Rule

不要：

为了 testability 把所有东西都 Protocol 化。

只有：

* 确实有多个实现
* 测试确实需要替换
* 存在明确边界

时使用 Protocol。

⸻

39. Dependency Injection

使用简单：

Initializer Injection

或者 SwiftUI 自身合适的 Environment 机制。

不要引入第三方 DI Framework。

⸻

40. Swift Concurrency

耗时操作不能阻塞 MainActor。

合理使用：

async/await
Task
actor

例如：

* Workspace scan
* hashing
* indexing
* database
* file processing

不要为了“并发架构”创建复杂任务系统。

⸻

41. Logging

使用：

OSLog
Logger

建议 category：

Workspace
FileSystem
Notes
Editor
Database
Index
Search
Watcher

禁止生产代码大量：

print(...)

⸻

42. Error Handling

禁止使用：

try!

处理正常运行时操作。

也不要因为普通错误：

fatalError()

至少正确处理：

* permission failure
* missing file
* database error
* malformed Markdown metadata
* attachment copy failure
* workspace unavailable

错误优先：

log
+
recover if possible
+
show user when necessary

⸻

43. Tests

测试真正重要的核心逻辑。

重点测试：

Workspace

* bookmark
* restore

Files

* create
* rename
* move
* delete
* atomic save

Markdown

* Front Matter
* missing Front Matter
* malformed Front Matter
* title
* tags
* Chinese content

Attachments

* copy
* duplicate filename
* relative path

Index

* insert
* update
* delete
* rebuild

Search

* English
* Chinese
* mixed
* title
* filename
* tags
* content

不要为了测试覆盖率写大量无价值 UI Tests。

⸻

44. Explicitly Out of Scope

除非用户以后明确修改本文件，否则禁止实现：

* Cloud Sync
* Account
* Login
* Collaboration
* AI
* LLM
* Embeddings
* Vector Database
* Graph View
* Backlinks
* Wikilinks
* Plugin System
* Theme Marketplace
* Web App
* Windows
* Linux
* Mobile App
* Version History
* Git Integration
* Publishing
* Calendar
* Tasks
* Kanban
* Database Notes
* OCR
* Browser Extension
* Web Clipper
* Complex Import System
* Complex Export System
* Custom Markdown Language

⸻

45. Scope Discipline

这是 Coding Agent 必须遵守的规则。

每次只实现：

Current Task

禁止：

* 顺便实现下一个 Task
* 顺便增加未来 Feature
* 顺便重构无关模块
* 顺便建立未来架构
* 顺便增加 dependency

如果发现：

下一阶段这样做可能更方便。

忽略。

等下一阶段真正开始后再处理。

⸻

46. No Premature Preparation

例如 Current Task 是：

T021 — Display Workspace Tree

禁止提前：

创建 SQLite
创建 SearchService
集成 Editor
实现 FileWatcher
创建 Sync abstraction

即使只创建空文件、Protocol、TODO placeholder 也禁止。

⸻

47. Minimal Diff Rule

修改已有项目时：

优先：

Smallest Correct Diff

开始前先阅读现有代码。

不要因为你认为另一套架构“更优雅”而重写已经工作的代码。

⸻

48. Dependency Rule

添加第三方 dependency 前必须确认：

1. 当前任务真正需要
2. Apple 原生 API 不适合
3. dependency 成熟
4. 能明显降低实现复杂度

禁止为了几行 helper 引入 Package。

⸻

49. Current Task Rule

必须在下面的 Task List 中找到：

CURRENT

标记。

一次只能存在一个 CURRENT Task。

只实现这个 Task。

如果没有 CURRENT：

不要自行选择 Task。

停止并告诉用户需要指定 Current Task。

⸻

50. Task Status

状态只有：

TODO
CURRENT
DONE
BLOCKED

含义：

TODO
尚未开始
CURRENT
当前唯一允许开发的任务
DONE
已经实现、编译并验证
BLOCKED
存在阻塞问题

只有满足 Definition of Done 才能改成 DONE。

⸻

51. Task List

⸻

M0 — Project Foundation

目标：

建立最小可运行 macOS 项目。

T001 — Create macOS Project

Status:

DONE

实现：

* 创建 Minne macOS SwiftUI App
* Bundle/Application 基础配置
* App 能启动
* 创建最基本 ContentView

不要实现：

* Workspace
* SQLite
* Editor
* Search

⸻

T002 — Basic Project Structure

Status:

DONE

实现当前真正需要的基础目录：

App
Views

以及必要的基础代码组织。

不要提前创建未来模块空目录。

⸻

T003 — Basic NavigationSplitView

Status:

DONE

实现：

Sidebar
+
Detail

只需要基础 shell。

Detail 可以显示 Empty State。

⸻

T004 — Logging

Status:

DONE

添加统一 Logger 基础设施。

不要创建复杂 Logging Framework。

⸻

M1 — Workspace

目标：

能够选择并重新打开用户 Workspace。

T010 — Select Workspace

Status:

DONE

使用 macOS 系统目录选择能力。

用户只能选择目录。

选择后 App 能获得 Workspace URL。

⸻

T011 — Security-Scoped Bookmark

Status:

DONE

为选择的 Workspace 创建并保存 Security-Scoped Bookmark。

⸻

T012 — Restore Workspace

Status:

DONE

App 启动时恢复 Bookmark。

处理 stale bookmark。

⸻

T013 — Initialize Minne Directory

Status:

DONE

创建：

.minne/

如果已经存在则复用。

不要创建 SQLite。

⸻

M2 — Workspace Files

目标：

能够浏览 Workspace 中真实的文件和目录。

T020 — Scan Workspace

Status:

DONE

递归扫描 Workspace。

识别：

directory
*.md

忽略：

.minne
*.files

暂时不要索引内容。

⸻

T021 — Display Workspace Tree

Status:

DONE

Sidebar 显示真实目录树。

显示：

* folders
* Markdown notes

不要实现虚拟目录。

⸻

T022 — Create Folder

Status:

DONE

支持创建真实文件夹。

⸻

T023 — Create Markdown Note

Status:

DONE

创建：

*.md

此阶段先允许创建最小 Markdown。

Front Matter 在后续任务实现。

⸻

T024 — Rename Folder

Status:

DONE

重命名真实目录。

⸻

T025 — Rename Note

Status:

DONE

重命名 Markdown 文件。

附件 rename 后续单独实现。

⸻

T026 — Move Note

Status:

DONE

支持 Markdown 在 Workspace 内移动。

⸻

T027 — Delete Note

Status:

DONE

删除 Markdown。

删除行为必须经过用户确认。

暂时不要设计 Trash System。

⸻

T028 — Delete Folder

Status:

DONE

删除目录。

非空目录必须明确提示风险。

不要静默递归删除。

⸻

T029 — Atomic Save

Status:

DONE

实现安全 Markdown write。

增加相关测试。

⸻

M3 — Markdown Domain

目标：

建立 Minne Markdown 最小数据规范。

T030 — Front Matter Parser

Status:

DONE

解析：

id
tags
created
updated

⸻

T031 — Stable Note ID

Status:

DONE

新建 Minne Note 时生成 ULID。

确保 rename/move 不改变 ID。

⸻

T032 — Create Note Metadata

Status:

DONE

新建笔记时生成：

---
id:
tags: []
created:
updated:
---

⸻

T033 — Parse Note Title

Status:

DONE

规则：

first H1
→
filename fallback

⸻

T034 — Parse Tags

Status:

DONE

读取 Front Matter tags。

⸻

T035 — Extract Plain Text

Status:

DONE

从 Markdown 提取适合全文搜索的 plain text。

不要实现复杂 Markdown rendering。

⸻

M4 — SQLite Index

目标：

建立可完全重建的本地搜索索引。

T040 — Add GRDB

Status:

DONE

仅此阶段添加 GRDB dependency。

⸻

T041 — Initialize SQLite

Status:

DONE

创建：

.minne/index.sqlite

⸻

T042 — Notes Schema

Status:

DONE

创建 notes 表。

只添加当前真正需要的字段。

⸻

T043 — Tags Schema

Status:

DONE

创建：

tags
note_tags

⸻

T044 — FTS5 Schema

Status:

DONE

创建全文索引。

验证目标 macOS SQLite 对所选 tokenizer 的支持。

⸻

T045 — Index Single Note

Status:

DONE

实现：

Markdown
→
ParsedNote
→
SQLite

⸻

T046 — Update Indexed Note

Status:

DONE

修改 Markdown 后正确更新索引。

⸻

T047 — Remove Indexed Note

Status:

DONE

文件删除后可以删除对应 index。

此阶段不实现 watcher。

⸻

T048 — Rebuild Index

Status:

DONE

实现：

Workspace
→
scan Markdown
→
rebuild SQLite

绝对不能修改 Markdown。

⸻

T049 — Incremental Startup Index

Status:

DONE

使用：

relative path
mtime
size
hash

判断变化。

避免每次全部重新索引。

⸻

M5 — Search

目标：

用户能够快速搜索所有 Markdown。

T050 — Basic FTS Search

Status:

DONE

实现基本 SearchService。

⸻

T051 — Search Title

Status:

DONE

⸻

T052 — Search Filename

Status:

DONE

⸻

T053 — Search Tags

Status:

DONE

⸻

T054 — Search Content

Status:

DONE

⸻

T055 — Chinese Search

Status:

DONE

验证：

状态机
实现方案
Swift并发

等查询。

⸻

T056 — Search Ranking

Status:

DONE

实现简单 ranking：

title
>
filename
>
tags
>
content
>
path

不要开发自定义复杂 ranking engine。

⸻

T057 — Search UI

Status:

DONE

实现简单全局搜索 UI。

显示：

title
path
snippet

⸻

T058 — Open Search Result

Status:

DONE

点击 Search Result 打开对应 Note。

⸻

M6 — Markdown Editor

目标：

能够真正编辑 Markdown。

T060 — WKWebView Editor Container

Status:

DONE

建立 SwiftUI ↔ WKWebView 基础容器。

暂时不集成复杂 Editor。

⸻

T061 — Integrate WYSIWYG Markdown Editor

Status:

DONE

集成选定 Markdown-first editor。

要求最终输出 Markdown。

⸻

T062 — Load Markdown

Status:

DONE

Swift 将 Markdown 加载进 Editor。

⸻

T063 — Editor Change Bridge

Status:

DONE

Editor 修改后将 Markdown 返回 Swift。

⸻

T064 — Save Edited Markdown

Status:

DONE

使用之前实现的 FileService / atomic save。

⸻

T065 — Auto Save

Status:

DONE

实现 debounce autosave。

默认约：

750ms

⸻

T066 — Update Index After Save

Status:

DONE

保存成功后更新该 Note 的本地 SQLite index。

注意：

这叫：

Index Update

不叫 Sync。

⸻

M7 — Tags

目标：

能够简单管理 Markdown tags。

T070 — Display Note Tags

Status:

DONE

显示当前 Note Front Matter tags。

⸻

T071 — Add Tag

Status:

DONE

添加 tag 后修改 Markdown Front Matter。

然后刷新 index。

⸻

T072 — Remove Tag

Status:

DONE

⸻

T073 — Sidebar Tags

Status:

DONE

Sidebar 显示已有 Tags。

⸻

T074 — Filter by Tag

Status:

DONE

点击 Tag 显示对应 Notes。

不要开发复杂 tag query language。

⸻

M8 — Attachments

目标：

支持拖拽本地附件。

T080 — Attachment Directory

Status:

DONE

为 Note 创建：

<note>.files/

⸻

T081 — Copy Attachment

Status:

DONE

拖入文件后复制到附件目录。

⸻

T082 — Duplicate Filename

Status:

DONE

自动处理：

image.png
image-1.png
image-2.png

⸻

T083 — Image Drop

Status:

DONE

拖入图片后：

复制文件并插入 Markdown image relative path。

⸻

T084 — Generic File Drop

Status:

DONE

PDF/ZIP/TXT 等：

插入 Markdown link。

⸻

T085 — Render Local Images

Status:

DONE

确保 Editor 正确显示 Workspace 中的本地图片。

⸻

T086 — Rename Note Attachments

Status:

DONE

Note rename 时：

A.md
A.files/
→
B.md
B.files/

并安全更新 Markdown relative paths。

⸻

M9 — External Local Changes

注意：

只有完成前面核心功能后才实现这个 Milestone。

目标：

检测其他本地程序对 Workspace 的修改。

这不是 Sync。

T090 — Workspace File Watcher

Status:

DONE

监听本地 Workspace 文件变化。

⸻

T091 — External Note Created

Status:

DONE

发现新的 .md 后更新 UI 和 index。

⸻

T092 — External Note Modified

Status:

DONE

外部修改 Markdown 后更新 index。

⸻

T093 — External Note Deleted

Status:

DONE

⸻

T094 — External Rename / Move

Status:

DONE

⸻

T095 — Open Note External Change

Status:

DONE

当前打开的 Note 被外部修改时：

如果没有未保存编辑：

reload

如果存在未保存编辑：

show conflict warning

不要静默覆盖。

⸻

M10 — Essential macOS Polish

这里只做真正影响使用的基础体验。

T100 — New Note Shortcut

Status:

DONE

例如：

⌘N

⸻

T101 — Search Shortcut

Status:

DONE

实现合理的快速搜索快捷键。

依赖 macOS 15.0 部署目标：

⌘F

⸻

T102 — Save Shortcut

Status:

DONE
⌘S

⸻

T103 — Context Menu

Status:

DONE

Sidebar 基础：

New Note
New Folder
Rename
Delete

（文件夹内右键、空白区右键均支持 New Note / New Folder）

⸻

T104 — Empty States

Status:

DONE

只实现必要 Empty State。

（选中文件夹空态、搜索无结果空态、未选笔记/无 Workspace 空态）

⸻

T105 — Basic Error Presentation

Status:

DONE

让重要错误能够被用户看到。

不要开发复杂 Notification System。

（单一错误 alert，覆盖保存、新建、重命名、移动、删除、标签、附件、选工作区失败）

⸻

M11 — Local Index Correctness

来源：

代码 Review 发现的问题（非用户可见功能的扩张，不违反 §52 STOP PRODUCT EXPANSION —— 这些是既有功能的正确性修复）。

T106 — Fix reconcile Orphan FTS Rows

Status:

DONE

问题：

`IndexUpdater.reconcile` 对「启动时检测到已删除的 .md 文件」只执行：

DELETE FROM notes WHERE relative_path = ?

没有删除对应的 note_fts 行（note_fts 是独立 FTS5 表，无触发器级联）。

后果（已用 SQLite 复现）：

已删除笔记的 FTS 行仍可被 MATCH 命中；SQLite 复用 rowid 后，新插入笔记被错误关联到已删笔记残留的 FTS 行（title/snippet 错配）。

要求：

reconcile 的 toDelete 逻辑改用与 IndexService.remove 相同的清理方式：

DELETE FROM note_fts WHERE rowid IN (SELECT rowid FROM notes WHERE relative_path = ?)
DELETE FROM notes WHERE relative_path = ?

并补一个测试：

testReconcileRemovesDeletedFileAlsoClearsFTS

（现有 testReconcileRemovesDeletedFile 只断言 notes 行、未断言 FTS 行）

Definition of Done：

- 删除文件的 reconcile 同时清理 notes + note_fts
- 新增测试验证 FTS 行被清除
- Build + 相关测试通过

⸻

T107 — Stable ID for notes missing Front Matter id

Status:

DONE

问题：

ParsedNote.init 使用：

id = fm?.id ?? NoteID.generate()

对：

Minne 自己创建的笔记（T032：写入稳定 id）

无影响。

对：

外部创建且无 Front Matter id 的笔记（T091/T092 支持的场景）

每次重新 parse 都会生成新 ULID：

首次 reconcile → 用 ULID-A 入索引。

随后外部修改 → updateFile 重新 parse → 生成 ULID-B → UPDATE notes WHERE id = B 匹配 0 行 → 抛 noteNotIndexed → 索引不更新。

结论：

外部创建的笔记被外部修改后，正文/标题的变化不会反映到搜索索引（T092 直接踩中）。

要求：

让无 id 笔记的 id 保持稳定，不再「每次 parse 生成新 ULID」。

优先考虑：

IndexUpdater.updateFile （和 reconcile 的 modified 分支）用 relative_path 定位既有行，已入库的 id 作为该笔记的稳定 id；只有真正的新文件才生成新 ULID。

并补测试：

- 无 id 笔记首次索引后，修改正文，updateFile 能更新成功（不再抛 noteNotIndexed）
- 无 id 笔记的 id 两次 parse 稳定一致

Definition of Done：

- 无 Front Matter id 的笔记在外部修改后索引能正确更新
- 补覆盖上述场景的测试
- Build + 相关测试通过

⸻

T108 — Bookmark Read-Only Access

Status:

DONE

问题：

WorkspaceManager.createBookmark 使用：

options: [.withSecurityScope, .securityScopeAllowOnlyReadAccess]

只读权限。

Minne 需要写入用户笔记（保存/重命名/删除/附件）。

当前开发构建无沙盒（Info.plist 无 Entitlements，isSandboxed == false），只读 bookmark 不生效，所以现在不报错。

但一旦启用 App Sandbox，只读 bookmark 会让所有写入操作失败。

要求：

移除 .securityScopeAllowOnlyReadAccess，改用可读写 bookmark。

Definition of Done：

- createBookmark 不再使用只读 option
- 现有 WorkspaceBookmarkTests 仍通过
- Build + 相关测试通过

⸻

T109 — Watcher mtime Precision Alignment

Status:

DONE

Reason：

- WorkspaceWatcher 把 mtime 截断为 秒级 Int64。
- IndexUpdater.reconcile 用 .timeIntervalSince1970（Double 亚秒）。

同秒内二次修改（size 相同）watcher 可能漏报 diff 触发；

两套刻度不一致。

要求：

统一 watcher 与 reconciler 的 mtime 取整/比较粒度，使同秒修改也能被正确识别。

（若实现复杂可先用 亚秒比较，避免引入额外状态。）

Definition of Done：

- watcher 使用与 reconcile 一致的 mtime 精度
- Build + 测试通过

⸻

T110 — PlainText 过度清洗

Status:

DONE

Reason：

PlainTextExtractor.stripInlineMarkdown 用正则 #"[*_~]"# 移除 所有 * _ ~ 字符。

后果：

正文字段中出现的下划线/星号（如 foo_bar、2*3）也被清除，导致搜索结果匹配度下降。

要求：

- 只在 Markdown 语境（加粗/斜体标记等）移除 标记，不 mechanically 移除所有 * _ ~ 字符。

- 补 PlainTextTests。

Definition of Done：

- 正文中普通下划线/星号保留
- Markdown 语法标点仍被剥离
- Build + 相关测试通过

⸻

M12 — Workspace UX Polish

目标：

修复 E2E 测试发现的三处基础 UX 问题：工作区可切换、新建笔记即打开、侧栏不显示 .md 后缀。

T111 — Switch Workspace

Status:

DONE

Reason：

选中工作区后没有任何入口再选择/切换工作区。「Select Workspace…」按钮只在空态显示。

要求：

- 提供一个在已有工作区时也能切换的入口（工具栏或菜单）。
- 复用已有 WorkspaceManager.selectWorkspace()。
- 切换成功后重建索引、刷新侧栏。

Definition of Done：

- 已有工作区状态下可触发切换
- 切换后侧栏/搜索指向新工作区
- Build + 相关测试通过

⸻

T112 — Open Note After Create

Status:

DONE

Reason：

createNote 成功后未更新 selectedItem，新建笔记在侧栏出现但不进编辑器。

要求：

- 新建成功后自动选中并打开该笔记。

Definition of Done：

- 新建笔记后编辑器立即显示该笔记
- Build + 相关测试通过

⸻

T113 — Hide .md Extension in Sidebar

Status:

DONE

Reason：

侧栏 Text(item.name) 直接渲染完整文件名含 .md 后缀。

要求：

- 侧栏笔记标题不显示 .md 后缀（标题规则参考 §11）。

Definition of Done：

- 侧栏笔记显示去尾缀文件名
- 编辑/重命名仍作用于真实文件
- Build + 相关测试通过

⸻

52. MVP Completion Boundary

当：

M0
到
M8

核心任务完成后：

Minne 已经达到第一版 MVP 的核心目标。

M9：

External Local Changes

属于增强可靠性能力。

M10：

只允许基础 macOS polish。

完成这些以后：

STOP PRODUCT EXPANSION.

不要自行新增 Milestone。

⸻

53. Do Not Add Tasks Automatically

Coding Agent 禁止自行向 Task List 添加：

* AI
* Sync
* Favorites
* Backlinks
* Graph
* Plugins
* History
* Cloud
* Mobile
* Collaboration
* Templates
* Themes
* Export
* Import

如果发现一个真正必要但 Task List 缺失的问题：

不要直接实现。

在结果中报告：

Suggested Task:
Reason:
Required for:

等待用户决定是否加入 Roadmap。

⸻

54. Current Task

当前唯一允许执行：

T113 — Hide .md Extension in Sidebar — DONE

等待用户指定下一个 Current Task。

不要自行选择任务。

⸻

55. Agent Execution Protocol

每次收到开发指令后：

Step 1 — Read

完整阅读：

AGENTS.md

⸻

Step 2 — Inspect

检查当前 repository：

git status
project structure
existing code
dependencies
tests

理解现状后再修改。

⸻

Step 3 — Confirm Current Task Internally

找到：

CURRENT

任务。

一次只能有一个 CURRENT。

如果：

0 CURRENT

停止。

如果：

>1 CURRENT

停止并报告配置错误。

⸻

Step 4 — Implement

只实现 Current Task。

使用：

Smallest Correct Diff

⸻

Step 5 — Build

实际编译项目。

不要只根据代码看起来正确就宣称完成。

如果有编译错误：

修复。

⸻

Step 6 — Test

运行当前任务相关测试。

如果当前阶段没有合理的自动测试：

至少进行 Build Validation。

不要为了满足流程写无价值测试。

⸻

Step 7 — Review Diff

执行并检查：

git diff

确认：

* 没有无关修改
* 没有未来 Feature
* 没有 Sync
* 没有无意义 abstraction
* 没有不必要 dependency

⸻

Step 8 — Update Task

只有满足 Definition of Done：

才能：

CURRENT
→
DONE

⸻

Step 9 — STOP

完成后停止。

禁止自动把下一个 TODO 改成 CURRENT。

Current Task 由用户指定。

⸻

56. Definition of Done

Task 只有满足：

Required functionality implemented
+
Project builds
+
Relevant tests pass
+
No obvious data loss risk
+
No unrelated features
+
No unnecessary abstractions
+
Diff reviewed

才能标记：

DONE

⸻

57. Response Format After Completing Task

完成任务后，只需要报告：

Task:
Status:
Implemented:
- ...
Validation:
- Build: PASS/FAIL
- Tests: PASS/FAIL
Files Changed:
- ...
Notes:
- 只有真正重要的信息
Next:
Waiting for next task.

不要写长篇总结。

不要提出十几个未来优化建议。

⸻

58. Final Instruction

Minne 的目标不是功能最多。

目标是：

简单、可靠、快速、用户拥有自己的数据。

当你犹豫：

要不要顺便实现这个？

默认答案：

不要。

当你犹豫：

要不要为了以后设计一层 abstraction？

默认答案：

不要。

当你犹豫：

这个功能是不是当前 Task 必需？

如果不是：

不要实现。

严格按照 Task List 和 Current Task 工作。

现在检查 repository。

只执行：

T113 — Hide .md Extension in Sidebar

完成、编译、验证、更新 T113 状态，然后 STOP。