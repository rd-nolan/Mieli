/**
 * Mieli Markdown editor frontend — T061.
 *
 * Boots a Milkdown (ProseMirror) WYSIWYG editor inside the WKWebView host
 * page and exposes a minimal global API that the native side (EditorBridge)
 * calls. AGENTS §8: the editor ultimately *produces Markdown*.
 *
 * Swift → frontend and frontend → Swift run through plain globals here;
 * the WKScriptMessage channel is wired in T063.
 */

import { Editor, rootCtx, defaultValueCtx, editorViewCtx, inputRulesCtx, SchemaReady } from "@milkdown/core";
import { commonmark } from "@milkdown/preset-commonmark";
import { serializerCtx, parserCtx, remarkStringifyOptionsCtx, prosePluginsCtx, remarkPluginsCtx } from "@milkdown/core";
import { Plugin, TextSelection } from "@milkdown/prose/state";
import { exitCode } from "@milkdown/prose/commands";
import { $markSchema, $nodeSchema } from "@milkdown/utils";
import remarkGfm from "remark-gfm";
import { visit } from "unist-util-visit";
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

// Milkdown's commonmark preset registers as-you-type rules that turn a
// leading prefix (`# `, `- `, `> `, `1. ` …) into a heading/list/quote and
// swallow the prefix as soon as it's typed. For a "type raw Markdown, convert
// on Enter" experience we disable those *block-level wrapping* rules so the
// prefix stays visible while typing; conversion happens on Enter (blockOnEnter
// below). Inline formats (bold, italic, code…) are left alone.
const BLOCK_WRAP_RULES = [
  "^(?<hashes>#+)\\s$",        // heading
  "^\\s*>\\s$",                // blockquote
  "^\\s*([-+*])\\s$",          // bullet list
  "^\\s*(\\d+)\\.\\s$",        // ordered list
];
function isBlockWrapInputRule(rule) {
  if (!rule?.match) return false;
  const src = String(rule.match.source);
  return BLOCK_WRAP_RULES.includes(src);
}
const disableBlockWrapInputRules = (ctx) => async () => {
  await ctx.wait(SchemaReady);
  ctx.update(inputRulesCtx, (rules) => rules.filter((r) => !isBlockWrapInputRule(r)));
  return () => {};
};

// Absolute document position just past the innermost text of `node` placed at
// `startPos`. Drills through wrapper nodes (list_item → paragraph → text…)
// so the caret lands exactly after the content, enabling natural list splits.
function caretAfterText(startPos, node) {
  let pos = startPos;
  let cur = node;
  while (cur.childCount) {
    pos += 1;
    cur = cur.child(0);
  }
  // `cur` is the leaf text node; its nodeSize equals the text length (no +2
  // wrapper delimiters), so the caret sits right after the last character.
  return pos + cur.nodeSize;
}

// remark-gfm marks task-list items with `checked`. Give those items their own
// editor node so the checkbox state is visible while remaining ordinary GFM
// Markdown when serialized.
const taskListGuard = () => (tree) => {
  visit(tree, "listItem", (node) => {
    if (node.checked === null || node.checked === undefined) return;
    node.type = "taskItem";
  });
};

const taskItemSchema = $nodeSchema("task_item", () => ({
  group: "listItem",
  content: "paragraph block*",
  defining: true,
  attrs: {
    checked: { default: false, validate: "boolean" },
    spread: { default: true, validate: "boolean" },
  },
  parseDOM: [{
    tag: "li[data-task-item]",
    getAttrs: (dom) => ({
      checked: dom.dataset.checked === "true",
      spread: dom.dataset.spread === "true",
    }),
  }],
  toDOM: (node) => [
    "li",
    {
      "data-task-item": "true",
      "data-checked": String(node.attrs.checked),
      "data-spread": String(node.attrs.spread),
    },
    ["input", {
      type: "checkbox",
      checked: node.attrs.checked ? "checked" : undefined,
      disabled: "disabled",
      "aria-label": node.attrs.checked ? "Completed task" : "Incomplete task",
      contenteditable: "false",
    }],
    ["div", { class: "task-content" }, 0],
  ],
  parseMarkdown: {
    match: (node) => node.type === "taskItem",
    runner: (state, node, type) => {
      state.openNode(type, {
        checked: Boolean(node.checked),
        spread: node.spread ?? true,
      });
      state.next(node.children);
      state.closeNode();
    },
  },
  toMarkdown: {
    match: (node) => node.type.name === "task_item",
    runner: (state, node) => {
      state.openNode("listItem", undefined, {
        checked: Boolean(node.attrs.checked),
        spread: node.attrs.spread,
      });
      state.next(node.content);
      state.closeNode();
    },
  },
}));

// remark-gfm produces table/tableRow/tableCell nodes. The CommonMark preset
// has no matching ProseMirror schema, so parsing a table previously rejected
// the entire document. These three small nodes keep the GFM structure editable
// and round-trip its column alignment metadata.
const tableCellSchema = $nodeSchema("table_cell", () => ({
  content: "inline*",
  isolating: true,
  parseDOM: [{ tag: "td" }, { tag: "th" }],
  toDOM: () => ["td", 0],
  parseMarkdown: {
    match: (node) => node.type === "tableCell",
    runner: (state, node, type) => {
      state.openNode(type);
      state.next(node.children);
      state.closeNode();
    },
  },
  toMarkdown: {
    match: (node) => node.type.name === "table_cell",
    runner: (state, node) => {
      state.openNode("tableCell");
      state.next(node.content);
      state.closeNode();
    },
  },
}));

const tableRowSchema = $nodeSchema("table_row", () => ({
  content: "table_cell+",
  parseDOM: [{ tag: "tr" }],
  toDOM: () => ["tr", 0],
  parseMarkdown: {
    match: (node) => node.type === "tableRow",
    runner: (state, node, type) => {
      state.openNode(type);
      state.next(node.children);
      state.closeNode();
    },
  },
  toMarkdown: {
    match: (node) => node.type.name === "table_row",
    runner: (state, node) => {
      state.openNode("tableRow");
      state.next(node.content);
      state.closeNode();
    },
  },
}));

const tableSchema = $nodeSchema("table", () => ({
  content: "table_row+",
  group: "block",
  isolating: true,
  attrs: { align: { default: [] } },
  parseDOM: [
    { tag: 'div[data-table-wrapper="true"]', contentElement: "tbody" },
    { tag: "table" },
  ],
  // The wrapper gives CSS a reliable positioning box for the external table
  // label. Pseudo-elements on <table> itself are not consistently rendered by
  // WebKit's table formatting context. The ProseMirror and Markdown node stays
  // the same semantic table.
  toDOM: () => [
    "div",
    { "data-table-wrapper": "true" },
    ["table", ["tbody", 0]],
  ],
  parseMarkdown: {
    match: (node) => node.type === "table",
    runner: (state, node, type) => {
      state.openNode(type, { align: Array.isArray(node.align) ? node.align : [] });
      state.next(node.children);
      state.closeNode();
    },
  },
  toMarkdown: {
    match: (node) => node.type.name === "table",
    runner: (state, node) => {
      state.openNode("table", undefined, { align: node.attrs.align });
      state.next(node.content);
      state.closeNode();
    },
  },
}));

// GFM delete/strike: supports `~~text~~` → <del>. Enabled by remarkGfm
// (registered via remarkPluginsCtx) which parses `~~` as a `delete` node; this
// mark then maps it to a ProseMirror mark and serializes it back as `~~text~~`.
const strikeSchema = $markSchema("strike", () => ({
  parseDOM: [
    { tag: "del" },
    { tag: "s" },
    { style: "text-decoration", getAttrs: (v) => (v === "line-through" ? null : false) },
  ],
  toDOM: (mark) => ["del", { class: "strike" }, 0],
  parseMarkdown: {
    match: (node) => node.type === "delete",
    runner: (state, node, markType) => {
      state.openMark(markType);
      state.next(node.children);
      state.closeMark(markType);
    },
  },
  toMarkdown: {
    match: (mark) => mark.type.name === "strike",
    runner: (state, mark) => {
      // NOTE: must return undefined (not the Serializer) so the text node
      // following this mark is still emitted into the open `delete` node.
      state.withMark(mark, "delete");
    },
  },
}));

// Converts a paragraph whose whole text is a Markdown block prefix into the
// corresponding block when the user presses Enter. So `# foo`, `- bar`,
// `> quote`, `1. item` show as-typed while editing and become a real heading /
// list item / quote / ordered item only on Enter. The prefix is stripped and
// the caret is placed after the text so further typing continues inside the
// new block (ProseMirror then handles list continuations).
const blockOnEnter = new Plugin({
  props: {
    handleKeyDown(view, event) {
      if (event.key !== "Enter" || event.shiftKey) return false;
      const { state } = view;
      const { $head, $anchor } = state.selection;

      // Handle code-block Return explicitly instead of relying on Milkdown's
      // downstream keymap. This keeps the caret inside the code block and also
      // replaces a same-block selection with the newline.
      if ($head.parent.type.name === "code_block" && $head.sameParent($anchor)) {
        if (event.metaKey) {
          return exitCode(state, view.dispatch);
        }
        const { from, to } = state.selection;
        view.dispatch(state.tr.insertText("\n", from, to).scrollIntoView());
        return true;
      }

      // Command-Return exits the entire enclosing heading, quote, list, or table.
      // Ordinary Return is left to Milkdown so each structure keeps its native
      // continuation behavior. Pick the outermost matching ancestor so nested
      // content returns to a normal top-level paragraph in one action.
      if (event.metaKey && state.selection.empty) {
        const exitBlockNames = new Set([
          "heading",
          "blockquote",
          "bullet_list",
          "ordered_list",
          "table",
        ]);
        let exitBlockDepth = null;
        for (let depth = 1; depth <= $head.depth; depth++) {
          if (exitBlockNames.has($head.node(depth).type.name)) {
            exitBlockDepth = depth;
            break;
          }
        }
        if (exitBlockDepth !== null) {
          const paragraphType = state.schema.nodes.paragraph;
          if (!paragraphType) return false;
          const insertAt = $head.after(exitBlockDepth);
          let tr = state.tr.insert(insertAt, paragraphType.create());
          tr = tr.setSelection(TextSelection.create(tr.doc, insertAt + 1));
          view.dispatch(tr.scrollIntoView());
          return true;
        }
      }

      // A newly created note initially contains only its H1. In WKWebView the
      // downstream keymap does not reliably create a following paragraph, so
      // make the end-of-heading transition explicit before the user types a
      // block prefix such as ```.
      if ($head.depth === 1
          && $head.parent.type.name === "heading"
          && state.selection.empty
          && $head.parentOffset === $head.parent.content.size) {
        const paragraphType = state.schema.nodes.paragraph;
        if (paragraphType) {
          const insertAt = $head.after();
          let tr = state.tr.insert(insertAt, paragraphType.create());
          tr = tr.setSelection(TextSelection.create(tr.doc, insertAt + 1));
          view.dispatch(tr.scrollIntoView());
          return true;
        }
      }

      // Normalize so that top-level blocks (depth 1) and blocks inside a list
      // item (depth ≥ 2) are both handled. We only run on plain paragraphs.
      const inListItem = (() => {
        for (let d = $head.depth; d > 0; d--) {
          const n = $head.node(d);
          if (n.type.name === "list_item") return true;
          if (n.type.isBlock && n.type.name !== "paragraph") break;
        }
        return false;
      })();
      const node = $head.parent;
      if (!node.isTextblock || node.type.name !== "paragraph") return false;
      if ($head.depth !== 1 && !inListItem) return false;
      const text = node.textContent;

      let body = "";
      let makeBlock = null; // () => ProseMirrorNode

      // Inside an existing list item the user may type `- ab` / `* ab` as the
      // item text; without handling, Milkdown escapes it to `\- ab`. When the
      // whole item paragraph is just a list-marker prefix plus content, strip
      // the marker so it stays a clean single item (no backslash escape).
      if (inListItem) {
        let mm = /^[-*+]\s+(.+)$/.exec(text) || /^\d+\.\s+(.+)$/.exec(text);
        if (mm && mm[1] && !/^\s*$/.test(mm[1])) {
          const itemText = mm[1];
          const pStart = $head.before($head.depth);
          const pEnd = pStart + node.nodeSize;
          let tr = state.tr.replaceWith(pStart, pEnd, node.type.create(null, state.schema.text(itemText)));
          tr = tr.setSelection(TextSelection.create(tr.doc, pStart + itemText.length));
          view.dispatch(tr);
          return true; // consume Enter; don't spawn an empty next item
        }
      }

      // Fenced heading: `### abc`
      let m = /^(#{1,6})(?:[ \t]+|)(.+)$/.exec(text);
      if (m && m[2] && !/^\s*$/.test(m[2])) {
        const headingType = state.schema.nodes.heading;
        if (headingType) {
          const level = m[1].length;
          body = m[2];
          makeBlock = () => headingType.create({ level }, headingType.schema.text(body));
        }
      }
if (!makeBlock && (m = /^\s*[-*+]\s+(.+)$/.exec(text)) && m[1] && !/^\s*$/.test(m[1])) {
        const schema = state.schema;
        if (schema.nodes.bullet_list && schema.nodes.list_item) {
          body = m[1];
          makeBlock = () =>
            schema.nodes.bullet_list.create(null,
              schema.nodes.list_item.create(null,
                schema.nodes.paragraph.create(null, schema.text(body))));
        }
      }
      // Ordered list: `1. abc`
      if (!makeBlock && (m = /^\s*\d+\.\s+(.+)$/.exec(text)) && m[1] && !/^\s*$/.test(m[1])) {
        const schema = state.schema;
        if (schema.nodes.ordered_list && schema.nodes.list_item) {
          body = m[1];
          makeBlock = () =>
            schema.nodes.ordered_list.create(null,
              schema.nodes.list_item.create(null,
                schema.nodes.paragraph.create(null, schema.text(body))));
        }
      }
      // Blockquote: `> abc`
      if (!makeBlock && (m = /^\s*>\s+(.+)$/.exec(text)) && m[1] && !/^\s*$/.test(m[1])) {
        const schema = state.schema;
        if (schema.nodes.blockquote) {
          body = m[1];
          makeBlock = () =>
            schema.nodes.blockquote.create(null,
              schema.nodes.paragraph.create(null, schema.text(body)));
        }
      }

if (!makeBlock) return false;
      const startPos = $head.before($head.depth);
      const endPos = startPos + node.nodeSize;
      const newBlock = makeBlock();
      let tr = state.tr.replaceWith(startPos, endPos, newBlock);
      // Put the caret right after the new block's leaf text (drill through
      // wrapper nodes to the innermost text node and sit just past it), so a
      // following Enter splits a fresh list item / paragraph naturally.
      const cursor = caretAfterText(startPos, newBlock);
      tr = tr.setSelection(TextSelection.create(tr.doc, cursor));
      view.dispatch(tr.scrollIntoView());
      return true;
    },
  },
});

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
      // Mount into the host placeholder. Pass the element itself (unambiguous)
      // rather than a selector string — Milkdown's rootCtx resolves a string
      // via document.querySelector, so a bare tag name like "editor" matches
      // nothing and the ProseMirror view silently mounts nowhere (T061 blank).
      const host = document.getElementById(HOST_ID);
      if (host) ctx.set(rootCtx, host);
      ctx.set(defaultValueCtx, "");
      // Keep unordered lists using `-` (remark-stringify default is `*`).
      // A `delete` markdown node (from `~~text~~` via remark-gfm) has no
      // default handler in Milkdown, so it would otherwise serialize to an
      // empty `~~~~`; add one that writes `~~text~~` (merging so Milkdown's
      // own text/strong/emphasis handlers are preserved).
      ctx.update(remarkStringifyOptionsCtx, (opts) => ({
        ...opts,
        bullet: "-",
        handlers: {
          ...opts.handlers,
          delete: (node, _, state, info) => {
            const marker = "~~";
            const exit = state.enter("delete");
            const tracker = state.createTracker(info);
            let value = tracker.move(marker);
            value += tracker.move(
              state.containerPhrasing(node, {
                before: value,
                after: marker,
                ...tracker.current(),
              })
            );
            value += tracker.move(marker);
            exit();
            return value;
          },
        },
      }));
      // Enable GFM (deletestrike `~~`, tables, autolinks) and guard task boxes.
      ctx.update(remarkPluginsCtx, (plugins) => [
        ...plugins,
        { plugin: remarkGfm },
        { plugin: taskListGuard },
      ]);
      // Report document changes to the native host (T063) + Enter-to-block.
      ctx.set(prosePluginsCtx, [
        ...ctx.get(prosePluginsCtx),
        changeNotifier,
        blockOnEnter,
      ]);
    })
    .use(commonmark)
    .use(disableBlockWrapInputRules)
    .use(strikeSchema)
    .use(taskItemSchema)
    .use(tableCellSchema)
    .use(tableRowSchema)
    .use(tableSchema)
    .create();

  // T085: absolutize local image srcs against the note's folder.
  const view = editor.action((ctx) => ctx.get(editorViewCtx));
  watchImages(view.dom);

  // YAML metadata keys that identify a real front-matter block; we only strip
  // when at least one is present so a normal top-of-file horizontal rule
  // (e.g. a bare `---`) is never mistaken for a header.
  const FM_KEY = /^([ \t]*)([a-zA-Z_-]+)\s*:/m;
  const FM_KEYS = ["id", "title", "tags", "created", "updated", "modified", "date"];

  // Split a YAML front-matter block off `text`. Accepts the canonical `---`/`---`
  // delimiters *and* a corrupted variant (AGENTS §9 data has historically been
  // mangled by an old editor bug into `***`…long-dashes). A block counts only
  // when its content is YAML-like (contains an `xxx:` line whose key is a known
  // metadata key). Returns { front, body }; `front` retains the original bytes
  // so it round-trips unchanged, `body` is what the editor edits/renders.
  function splitFrontMatter(md) {
    const text = String(md ?? "");
    const delims = "-\\*_"; // -, *, _ are all valid block fences
    // opening fence must be a run of a single kind at line start
    const pattern = new RegExp(
      "^([" + delims + "])\\1{2,}[ \\t]*\\r?\\n" + // fence: --- | *** | ___
      "([\\s\\S]*?)\\r?\\n" +
      "([" + delims + "])\\3{2,}[ \\t]*(?:\\r?\\n|$)",
      "i"
    );
    const m = pattern.exec(text);
    if (!m) return { front: "", body: text };
    const content = m[2];
    // Only treat as front matter when the block looks like YAML metadata.
    const keyHit = FM_KEY.test(content);
    const knownKey = keyHit && FM_KEYS.some((k) => new RegExp(`^[ \\t]*${k}\\s*:`, "m").test(content));
    if (!knownKey) return { front: "", body: text };

    const head = text.slice(0, m[0].length); // fences + content + trailing newline
    let body = text.slice(m[0].length);
    body = body.replace(/^\s*\r?\n/, ""); // drop blank separator before first line
    return { front: head, body };
  }
  function joinFrontMatter(front, body) {
    if (!front) return body;
    return front + "\n" + body;
  }
  let currentFrontMatter = ""; // original header block, restored on save

  // Minimal public API for the native side. Markdown is the single source of
  // truth: getMarkdown() returns what would be persisted, setMarkdown() loads it.
  window.mieliEditor = {
    /** Updates editor-owned labels without touching Markdown content. */
    setLanguage(language) {
      document.documentElement.lang = String(language).toLowerCase().startsWith("zh")
        ? "zh-Hans"
        : "en";
    },

    /** Moves keyboard focus back into the ProseMirror editor. */
    focus() {
      view.focus();
    },

    /** Current content serialized to Markdown ('' when empty). */
    getMarkdown() {
      const body = editor.action((ctx) => {
        return ctx.get(serializerCtx)(ctx.get(editorViewCtx).state.doc) || "";
      });
      return joinFrontMatter(currentFrontMatter, body);
    },

    /** Replaces the editor content with Markdown `md`. */
    setMarkdown(md) {
      // Strip the YAML header so it is neither shown nor editable in the
      // WYSIWYG view; the original block is restored on getMarkdown/save.
      const { front, body } = splitFrontMatter(md);
      currentFrontMatter = front || "";
      editor.action((ctx) => {
        const v = ctx.get(editorViewCtx);
        const doc = ctx.get(parserCtx)(body);
        const tr = v.state.tr.replaceWith(0, v.state.doc.content.size, doc.content);
        suppressNotify = true;
        v.dispatch(tr);
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
  console.error("[mieli-editor] boot failed", err);
});
