/**
 * Minne Markdown editor frontend — T061.
 *
 * Boots a Milkdown (ProseMirror) WYSIWYG editor inside the WKWebView host
 * page and exposes a minimal global API that the native side (EditorBridge)
 * calls. AGENTS §8: the editor ultimately *produces Markdown*.
 *
 * Swift → frontend and frontend → Swift run through plain globals here;
 * the WKScriptMessage channel is wired in T063.
 */

import { Editor, rootCtx, defaultValueCtx, editorViewCtx } from "@milkdown/core";
import { commonmark } from "@milkdown/preset-commonmark";
import { serializerCtx, parserCtx, remarkStringifyOptionsCtx, prosePluginsCtx } from "@milkdown/core";
import { Plugin } from "@milkdown/prose/state";
import "@milkdown/theme-nord/style.css";

const HOST_ID = "editor"; // matches the host placeholder in editor.html

// Fires a lightweight {type:"contentChanged"} to the native host whenever the
// document changes (typing, formatting…). Programmatic loads via setMarkdown()
// are suppressed so the host isn't told its own load changed the document.
const changeNotifier = new Plugin({
  view(view) {
    let previous = view.state.doc;
    return {
      update(view) {
        const current = view.state.doc;
        if (current === previous) return;
        previous = current;
        if (!suppressNotify && window.webkit?.messageHandlers?.editorContent) {
          window.webkit.messageHandlers.editorContent.postMessage({ type: "contentChanged" });
        }
      },
    };
  },
});

let suppressNotify = false;

/** Current Markdown content — absolute base URI of the open note's folder
 *  (e.g. `file:///…/工作/`), injected by the native side (T085). Relative
 *  image srcs from the note are resolved against it. */
let imageBaseURI = "";

// Pure: turn a relative image src (e.g. `./note.files/x.png`) into an
// absolute URI against the note's folder base. Already-absolute srcs pass
// through untouched.
function resolveImageSrc(baseURI, raw) {
  if (!baseURI || !raw) return raw;
  if (/^(https?:|data:|file:|\/)/i.test(raw)) return raw;
  return baseURI + encodeURI(raw.replace(/^\.\//, ""));
}

function patchImageSrc(img) {
  if (!imageBaseURI) return;
  const raw = img.getAttribute("src");
  if (!raw) return;
  const resolved = resolveImageSrc(imageBaseURI, raw);
  if (resolved !== raw) img.setAttribute("src", resolved);
}

// Rewrite relative <img src> so local attachments (e.g. `./note.files/x.png`)
// resolve to the open note's folder inside the workspace. Watch the live DOM
// instead of ProseMirror state: patch only presentation, never the doc.
function watchImages(root) {
  if (!root) return;
  const observerSupported = typeof MutationObserver !== "undefined";
  if (observerSupported) {
    new MutationObserver((muts) => {
      for (const m of muts) {
        if (m.type === "attributes" && m.attributeName === "src") {
          patchImageSrc(m.target);
        } else if (m.type === "childList") {
          m.addedNodes.forEach((n) => {
            if (n.nodeType === 1) {
              if (n.tagName === "IMG") patchImageSrc(n);
              n.querySelectorAll?.("img").forEach(patchImageSrc);
            }
          });
        }
      }
    }).observe(root, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["src"],
    });
  }
  root.querySelectorAll("img").forEach(patchImageSrc);
}

async function boot() {
  const editor = await Editor.make()
    .config((ctx) => {
      ctx.set(rootCtx, HOST_ID);
      ctx.set(defaultValueCtx, "");
      // Keep unordered lists using `-` (remark-stringify default is `*`).
      ctx.set(remarkStringifyOptionsCtx, { bullet: "-" });
      // Report document changes to the native host (T063).
      ctx.set(prosePluginsCtx, [...ctx.get(prosePluginsCtx), changeNotifier]);
    })
    .use(commonmark)
    .create();

  // Minimal public API for the native side. Markdown is the single source of
  // truth: getMarkdown() returns what would be persisted, setMarkdown() loads it.
  window.minneEditor = {
    /** Current content serialized to Markdown ('' when empty). */
    getMarkdown() {
      return editor.action((ctx) => {
        const md = ctx.get(serializerCtx)(ctx.get(editorViewCtx).state.doc);
        return md || "";
      });
    },

    /** Replaces the editor content with Markdown `md`. */
    setMarkdown(md) {
      editor.action((ctx) => {
        const view = ctx.get(editorViewCtx);
        const doc = ctx.get(parserCtx)(String(md ?? ""));
        const tr = view.state.tr.replaceWith(0, view.state.doc.content.size, doc.content);
        suppressNotify = true;
        view.dispatch(tr);
        suppressNotify = false;
      });
    },

    /** True once the editor is fully created and ready to hold content. */
    isReady() {
      return true;
    },

    /** Sets the base `file://` URI of the open note's folder (T085); relative
     *  image srcs are then absolutized live. `""` disables rewriting. */
    setImageBaseURI(base) {
      imageBaseURI = String(base ?? "");
      view.dom.querySelectorAll("img").forEach(patchImageSrc);
    },

    /** Reapply relative-img rewriting to every visible image (exposed for
     *  host + diagnostic use). */
    absolutizeImages() {
      view.dom.querySelectorAll("img").forEach(patchImageSrc);
    },

    /** Pure: resolve a relative img src against the note base URI (test aid). */
    resolveImageSrc(raw) {
      return resolveImageSrc(imageBaseURI, raw);
    },

    /** Inserts a Markdown fragment at the current cursor (e.g. an image link). */
    insertAttachment(md) {
      editor.action((ctx) => {
        const view = ctx.get(editorViewCtx);
        const doc = ctx.get(parserCtx)(String(md ?? ""));
        const tr = view.state.tr;
        // Replace the selection with the parsed fragment (renders WYSIWYG).
        tr.replaceWith(tr.selection.from, tr.selection.to, doc.content);
        suppressNotify = true;
        view.dispatch(tr);
        suppressNotify = false;
        view.focus();
      });
    },
  };

  // Forward a dropped local file to the native host, which copies it into the
  // note's `.files/` folder and returns the Markdown to insert (T83). We
  // leave the copy + relative-link work to Swift (AttachmentService); the
  // editor merely reports the drop and later receives the fragment to insert.
  const view = editor.action((ctx) => ctx.get(editorViewCtx));
  // T085: absolutize local image srcs against the note's folder.
  watchImages(view.dom);
  view.dom.addEventListener("drop", (e) => {
    const file = e.dataTransfer?.files?.[0];
    if (!file) return;
    // WKWebView (loaded via loadFileURL) exposes `file.path`; forwarded as-is.
    if (window.webkit?.messageHandlers?.editorContent) {
      window.webkit.messageHandlers.editorContent.postMessage({
        type: "attachmentDropped",
        path: file.path ?? "",
        name: file.name,
        isImage: file.type.startsWith("image/"),
      });
    }
    // Stop the editor from treating the drop as default HTML/paste handling.
    e.preventDefault();
  });

  // Notify the native host that page + editor are ready.
  if (window.webkit?.messageHandlers?.editorBridge) {
    window.webkit.messageHandlers.editorBridge.postMessage({ type: "editorReady" });
  }
}

boot().catch((err) => {
  console.error("[minne-editor] boot failed", err);
});