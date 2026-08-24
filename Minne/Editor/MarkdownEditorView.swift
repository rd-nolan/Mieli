import SwiftUI
import WebKit

/// SwiftUI host view that wraps the Markdown editor's WKWebView (T060).
///
/// This view is the Editor boundary (AGENTS §16): all `evaluateJavaScript`
/// into the Milkdown editor and every message it emits flow through this
/// view's `Coordinator`. T061 integrated the Milkdown WYSIWYG bundle; T062
/// loads a note's Markdown into the editor; T063 bridges document changes
/// back to Swift via a WKScriptMessage "contentChanged" event.
///
/// - `markdown`: content to display. Refresh on change (e.g. note selection).
/// - `onContentChanged`: called with the latest serialized Markdown when the
///   user edits the document. Persistence (autosave) is wired in T064.
struct MarkdownEditorView: NSViewRepresentable {
    let markdown: String
    var focusRequest = 0
    var languageIdentifier = "en"
    var onContentChanged: ((String) -> Void)? = nil
/// A local file dropped into the editor (T083). The closure builds the
    /// Markdown to insert (copy + relative link via AttachmentService) and
    /// calls the second argument to inject it at the cursor.
    var onAttachmentDropped: ((AttachmentDrop, @escaping (String) -> Void) -> Void)? = nil
    /// Absolute `file://` directory of the open note (T085). Relative image
    /// srcs in the note (`./note.files/x.png`) are absolutized against it.
    var imageBaseURL: URL? = nil
    /// Message handler exposed to the bundled page (see editor.bundle.js).
    static let changeHandlerName = "editorContent"

    /// A file dropped onto the editor, forwarded from the bundled page.
    struct AttachmentDrop {
        let path: String   // source file path (may be "" if unavailable)
        let name: String
        let isImage: Bool
    }

    func makeCoordinator() -> Coordinator { Coordinator(parent: self) }

    func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.userContentController.add(context.coordinator, name: MarkdownEditorView.changeHandlerName)

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = context.coordinator

        // Load the bundled editor host page (Milkdown mounts on #editor).
        if let url = Bundle.main.url(forResource: "editor", withExtension: "html") {
            webView.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
        }
        return webView
    }

    func updateNSView(_ nsView: WKWebView, context: Context) {
        context.coordinator.parent = self
        context.coordinator.sync(markdown: markdown, webView: nsView)
    }

    /// Bridge owner. Drives the WKWebView editor and receives its messages.
    ///
    /// All `evaluateJavaScript` and `WKScriptMessage` handling is confined
    /// here (AGENTS §16); the rest of the app never touches the web content.
    final class Coordinator: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
        /// Latest representable; `sync` re-reads it on SwiftUI refresh.
        var parent: MarkdownEditorView

        private var pendingMarkdown = ""
        private var injectedMarkdown: String? // last md actually pushed to JS
        private var injectedImageBase: String?  // last base URI pushed to JS
        private var injectedLanguage: String?
        private var appliedFocusRequest = 0
        private var didFinish = false
        private weak var hostView: WKWebView?
        private var injectTask: Task<Void, Never>?
        private var focusTask: Task<Void, Never>?
        private var languageTask: Task<Void, Never>?

        init(parent: MarkdownEditorView) {
            self.parent = parent
        }

        /// Called by `updateNSView` on every SwiftUI refresh, and after the
        /// page finishes loading. Pushes a new Markdown to the editor when the
        /// page is ready and the content actually differs.
        func sync(markdown: String, webView: WKWebView) {
            pendingMarkdown = markdown
            hostView = webView
            guard didFinish else { return }
            injectIfNeeded()
            injectLanguageIfNeeded()
            focusIfNeeded()
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            hostView = webView
            didFinish = true
            injectIfNeeded()
            injectLanguageIfNeeded()
            focusIfNeeded()
        }

        // MARK: - WKScriptMessageHandler

        func userContentController(_ userContentController: WKUserContentController,
                                   didReceive message: WKScriptMessage) {
            guard message.name == MarkdownEditorView.changeHandlerName,
                  let body = message.body as? [String: Any] else { return }

            switch body["type"] as? String {
            case "contentChanged":
                guard let webView = hostView, parent.onContentChanged != nil else { return }
                // Editor changed — read the current Markdown back to Swift (T063).
                let js = "window.minneEditor.getMarkdown()"
                webView.evaluateJavaScript(js) { [weak self] result, _ in
                    guard let self, let md = result as? String else { return }
                    self.parent.onContentChanged?(md)
                }

            case "attachmentDropped":
                // A local file was dropped into the editor (T083). Hand the raw
                // drop to the host; it copies the file and gives back the
                // Markdown to insert, which we inject at the cursor.
                guard let path = body["path"] as? String,
                      let name = body["name"] as? String,
                      let handler = parent.onAttachmentDropped else { return }
                let drop = AttachmentDrop(
                    path: path,
                    name: name,
                    isImage: (body["isImage"] as? Bool) ?? false)
                handler(drop) { [weak self] fragment in
                    self?.insertAttachment(fragment)
                }

            default:
                break
            }
        }

        // MARK: - Injection

        private func injectIfNeeded() {
            guard didFinish, let hostView else { return }
            // Push the absolute image base URI to the frontend (T085) so
            // relative img srcs resolve; independent of the markdown load.
            injectImageBaseIfNeeded(in: hostView)
            guard pendingMarkdown != injectedMarkdown else { return }
            let md = pendingMarkdown
            injectTask?.cancel()
            injectTask = Task { [weak self, weak hostView] in
                // Milkdown boots asynchronously; wait until its API exists.
                while !Task.isCancelled {
                    if let ready = await self?.isEditorReady(in: hostView), ready { break }
                    try? await Task.sleep(for: .milliseconds(40))
                }
                guard !Task.isCancelled else { return }
                await self?.setMarkdown(md, in: hostView)
                self?.injectedMarkdown = md
            }
        }

        private func injectImageBaseIfNeeded(in webView: WKWebView) {
            guard let baseURL = parent.imageBaseURL,
                  baseURL.absoluteString != injectedImageBase else { return }
            guard let data = try? JSONEncoder().encode(baseURL.absoluteString),
                  let encoded = String(data: data, encoding: .utf8) else { return }
            let js = "window.minneEditor.setImageBaseURI(\(encoded)); true;"
            webView.evaluateJavaScript(js)
            injectedImageBase = baseURL.absoluteString
        }

        private func injectLanguageIfNeeded() {
            guard didFinish,
                  parent.languageIdentifier != injectedLanguage,
                  let hostView else { return }
            let language = parent.languageIdentifier
            languageTask?.cancel()
            languageTask = Task { [weak self, weak hostView] in
                while !Task.isCancelled {
                    if let ready = await self?.isEditorReady(in: hostView), ready { break }
                    try? await Task.sleep(for: .milliseconds(40))
                }
                guard !Task.isCancelled, let hostView,
                      let data = try? JSONEncoder().encode(language),
                      let encoded = String(data: data, encoding: .utf8) else { return }
                _ = try? await hostView.evaluateJavaScript(
                    "window.minneEditor.setLanguage(\(encoded)); true;"
                )
                self?.injectedLanguage = language
            }
        }

        private func focusIfNeeded() {
            guard didFinish,
                  parent.focusRequest != appliedFocusRequest,
                  let hostView else { return }
            let request = parent.focusRequest
            focusTask?.cancel()
            focusTask = Task { [weak self, weak hostView] in
                while !Task.isCancelled {
                    if let ready = await self?.isEditorReady(in: hostView), ready { break }
                    try? await Task.sleep(for: .milliseconds(40))
                }
                guard !Task.isCancelled, let hostView else { return }
                _ = try? await hostView.evaluateJavaScript("window.minneEditor.focus(); true;")
                self?.appliedFocusRequest = request
            }
        }

        private func isEditorReady(in webView: WKWebView?) async -> Bool {
            guard let webView else { return false }
            let js = "typeof window.minneEditor !== 'undefined' && window.minneEditor.isReady()"
            let result = try? await webView.evaluateJavaScript(js) as? Bool
            return result == true
        }

        private func setMarkdown(_ md: String, in webView: WKWebView?) async {
            guard let webView else { return }
            // JSON-encode the string so it embeds safely (escapes quotes, etc.).
            guard let data = try? JSONEncoder().encode(md),
                  let encoded = String(data: data, encoding: .utf8) else { return }
            let js = "window.minneEditor.setMarkdown(\(encoded)); true;"
            _ = try? await webView.evaluateJavaScript(js)
        }

        /// Inject a Markdown fragment (e.g. an image link) at the editor cursor
        /// (T083). Runs async after the editor is ready.
        private func insertAttachment(_ fragment: String) {
            guard didFinish, let hostView else { return }
            guard let data = try? JSONEncoder().encode(fragment),
                  let encoded = String(data: data, encoding: .utf8) else { return }
            let js = "window.minneEditor.insertAttachment(\(encoded)); true;"
            hostView.evaluateJavaScript(js)
        }
    }
}
