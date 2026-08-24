import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";
import { JSDOM } from "jsdom";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const frontendRoot = path.resolve(__dirname, "..");
const sourcePath = path.join(frontendRoot, "src/index.js");
const editorHTMLPath = path.resolve(frontendRoot, "../Mieli/Resources/editor.html");

// Expose the real editor view and Mieli Enter plugin only in this in-memory
// verification bundle. Production globals and bundled resources stay unchanged.
const instrumentedSource = readFileSync(sourcePath, "utf8")
  .replace(
    "const blockOnEnter = new Plugin({",
    "const blockOnEnter = window.__mieliBlockOnEnter = new Plugin({",
  )
  .replace(
    "window.mieliEditor = {",
    "window.__mieliEditorView = view;\n  window.mieliEditor = {",
  );

const buildResult = await build({
  stdin: {
    contents: instrumentedSource,
    resolveDir: path.dirname(sourcePath),
    sourcefile: sourcePath,
  },
  bundle: true,
  format: "iife",
  outdir: "out",
  write: false,
});
const javascript = buildResult.outputFiles.find((file) => file.path.endsWith(".js"))?.text;
assert.ok(javascript, "instrumented editor JavaScript should build");

const dom = new JSDOM(readFileSync(editorHTMLPath, "utf8"), {
  url: "file:///mieli/editor.html",
  runScripts: "dangerously",
  beforeParse(window) {
    window.console = console;
  },
});
const { window } = dom;
window.webkit = {
  messageHandlers: {
    editorContent: { postMessage() {} },
  },
};

const script = window.document.createElement("script");
script.textContent = javascript;
window.document.body.appendChild(script);

await poll(() => window.mieliEditor?.isReady());

const editor = window.mieliEditor;
const view = window.__mieliEditorView;
const enterPlugin = window.__mieliBlockOnEnter;
assert.ok(view && enterPlugin, "instrumented editor internals should be available");

editor.setMarkdown("```swift\nlet value = 1\n```");
let codePosition;
view.state.doc.descendants((node, position) => {
  if (node.type.name === "code_block") codePosition = position;
});
assert.notEqual(codePosition, undefined, "fixture should parse as a code block");

const codeBlock = view.state.doc.nodeAt(codePosition);
const cursor = codePosition + 1 + codeBlock.textContent.length;
const selection = view.state.selection.constructor.near(view.state.doc.resolve(cursor));
view.dispatch(view.state.tr.setSelection(selection));

const handled = enterPlugin.props.handleKeyDown(view, {
  key: "Enter",
  shiftKey: false,
});

assert.equal(handled, true, "Mieli Enter handler should consume Return in a code block");
assert.equal(
  view.state.doc.nodeAt(codePosition).textContent,
  "let value = 1\n",
  "Return should append a newline inside the code block",
);
assert.match(
  editor.getMarkdown(),
  /```swift\nlet value = 1\n\n```/,
  "the inserted newline should survive Markdown serialization",
);
const serialized = editor.getMarkdown();
editor.setMarkdown(serialized);
assert.equal(
  editor.getMarkdown(),
  serialized,
  "the multiline code block should survive Markdown reload",
);

editor.setMarkdown("```swift\nlet value = 1\n```");
view.state.doc.descendants((node, position) => {
  if (node.type.name === "code_block") codePosition = position;
});
const exitCodeBlock = view.state.doc.nodeAt(codePosition);
const exitCursor = codePosition + 1 + exitCodeBlock.textContent.length;
view.dispatch(
  view.state.tr.setSelection(
    view.state.selection.constructor.near(view.state.doc.resolve(exitCursor)),
  ),
);
const exitHandled = enterPlugin.props.handleKeyDown(view, {
  key: "Enter",
  shiftKey: false,
  metaKey: true,
});
assert.equal(exitHandled, true, "Command-Return should exit a code block");
assert.equal(view.state.doc.childCount, 2, "exiting should append a sibling block");
assert.equal(
  view.state.doc.child(1).type.name,
  "paragraph",
  "Command-Return should place a normal paragraph after the code block",
);
assert.equal(
  view.state.selection.$head.parent.type.name,
  "paragraph",
  "the caret should move into the paragraph after the code block",
);
assert.equal(
  view.state.doc.firstChild.textContent,
  "let value = 1",
  "exiting should not change code block contents",
);

editor.setMarkdown("> quoted text");
let quotePosition;
view.state.doc.descendants((node, position) => {
  if (node.type.name === "blockquote") quotePosition = position;
});
assert.notEqual(quotePosition, undefined, "fixture should parse as a blockquote");
const quote = view.state.doc.nodeAt(quotePosition);
const quoteCursor = quotePosition + 2 + quote.firstChild.textContent.length;
view.dispatch(
  view.state.tr.setSelection(
    view.state.selection.constructor.near(view.state.doc.resolve(quoteCursor)),
  ),
);
const quoteExitHandled = enterPlugin.props.handleKeyDown(view, {
  key: "Enter",
  shiftKey: false,
  metaKey: true,
});
assert.equal(quoteExitHandled, true, "Command-Return should exit a blockquote");
assert.equal(view.state.doc.childCount, 2, "exiting should append a sibling block");
assert.equal(
  view.state.doc.firstChild.type.name,
  "blockquote",
  "the original blockquote should remain intact",
);
assert.equal(
  view.state.doc.firstChild.textContent,
  "quoted text",
  "exiting should not change blockquote contents",
);
assert.equal(
  view.state.doc.child(1).type.name,
  "paragraph",
  "Command-Return should place a normal paragraph after the blockquote",
);
assert.equal(
  view.state.selection.$head.parent.type.name,
  "paragraph",
  "the caret should move into the paragraph after the blockquote",
);

assertCommandExit({
  markdown: "## heading",
  text: "heading",
  blockType: "heading",
  cursorOffset: 2,
});
assertCommandExit({
  markdown: "- bullet item",
  text: "bullet item",
  blockType: "bullet_list",
});
assertCommandExit({
  markdown: "1. ordered item",
  text: "ordered item",
  blockType: "ordered_list",
});
assertCommandExit({
  markdown: "- outer item\n  - nested item",
  text: "nested item",
  blockType: "bullet_list",
});
assertCommandExit({
  markdown: "- [ ] task item",
  text: "task item",
  blockType: "bullet_list",
});
assertCommandExit({
  markdown: "| A | B |\n| --- | --- |\n| cell | value |",
  text: "cell",
  blockType: "table",
});

editor.setMarkdown("| A | B |\n| --- | --- |\n| cell | value |");
const originalTableDoc = view.state.doc;
const tableHTML = view.dom.querySelector('[data-table-wrapper="true"]')?.outerHTML;
assert.ok(tableHTML, "rendered table wrapper should be available for DOM round-trip");
editor.setMarkdown("");
const pasteEvent = new window.Event("paste", { bubbles: true, cancelable: true });
Object.defineProperty(pasteEvent, "clipboardData", {
  value: {
    files: [],
    items: [],
    getData(type) {
      return type === "text/html" ? tableHTML : "";
    },
  },
});
view.dom.dispatchEvent(pasteEvent);
assert.equal(pasteEvent.defaultPrevented, true, "table DOM should be accepted by the real paste parser");
assert.equal(view.state.doc.childCount, 1, "DOM reparse should keep one table block");
assert.equal(view.state.doc.firstChild.type.name, "table", "DOM reparse should keep the table node");
assert.equal(
  view.state.doc.textContent,
  originalTableDoc.textContent,
  "table DOM should preserve all cell content",
);

editor.setMarkdown("# New note");
const heading = view.state.doc.firstChild;
const headingCursor = 1 + heading.textContent.length;
view.dispatch(
  view.state.tr.setSelection(
    view.state.selection.constructor.near(view.state.doc.resolve(headingCursor)),
  ),
);
const headingHandled = enterPlugin.props.handleKeyDown(view, {
  key: "Enter",
  shiftKey: false,
});
assert.equal(headingHandled, true, "Mieli Enter handler should consume Return at a heading end");
assert.equal(view.state.doc.childCount, 2, "Return at a heading end should append a block");
assert.equal(
  view.state.doc.child(1).type.name,
  "paragraph",
  "the appended block should be a normal paragraph",
);

// An escaped fence parses as an ordinary paragraph whose visible text is ```;
// this reproduces the editor state immediately after typing three backticks.
editor.setMarkdown("\\`\\`\\`");
const fenceParagraph = view.state.doc.firstChild;
const fenceCursor = 1 + fenceParagraph.textContent.length;
view.dispatch(
  view.state.tr.setSelection(
    view.state.selection.constructor.near(view.state.doc.resolve(fenceCursor)),
  ),
);

dispatchReturn();
assert.ok(
  view.dom.querySelector("pre"),
  "the first Return after three backticks should create a code block",
);

view.dispatch(view.state.tr.insertText("let first = 1"));
dispatchReturn();
view.dispatch(view.state.tr.insertText("let second = 2"));
assert.equal(
  view.state.doc.firstChild.textContent,
  "let first = 1\nlet second = 2",
  "subsequent Return should allow multiline code input",
);
const typedMarkdown = editor.getMarkdown();
assert.match(typedMarkdown, /let first = 1\nlet second = 2/);
editor.setMarkdown(typedMarkdown);
assert.equal(editor.getMarkdown(), typedMarkdown, "typed multiline code should survive reload");

console.log("PASS  code-block Return, all block Command-Return exits, and Markdown round-trip");

function assertCommandExit({ markdown, text, blockType, cursorOffset = text.length }) {
  editor.setMarkdown(markdown);
  const originalMarkdown = editor.getMarkdown();
  let textblockPosition;
  view.state.doc.descendants((node, position) => {
    if (textblockPosition === undefined && node.isTextblock && node.textContent.includes(text)) {
      textblockPosition = position;
    }
  });
  assert.notEqual(textblockPosition, undefined, `${blockType} fixture should contain ${text}`);
  const textblock = view.state.doc.nodeAt(textblockPosition);
  const cursorPosition = textblockPosition + 1 + textblock.textContent.indexOf(text) + cursorOffset;
  view.dispatch(
    view.state.tr.setSelection(
      view.state.selection.constructor.near(view.state.doc.resolve(cursorPosition)),
    ),
  );
  const handled = enterPlugin.props.handleKeyDown(view, {
    key: "Enter",
    shiftKey: false,
    metaKey: true,
  });
  assert.equal(handled, true, `Command-Return should exit ${blockType}`);
  assert.equal(view.state.doc.childCount, 2, `${blockType} exit should append a sibling block`);
  assert.equal(view.state.doc.firstChild.type.name, blockType, `${blockType} should remain intact`);
  assert.equal(view.state.doc.child(1).type.name, "paragraph", `${blockType} exit should append a paragraph`);
  assert.equal(view.state.selection.$head.parent.type.name, "paragraph", `${blockType} exit should move the caret`);
  assert.ok(view.state.doc.firstChild.textContent.includes(text), `${blockType} contents should be preserved`);
  assert.ok(editor.getMarkdown().startsWith(originalMarkdown.trim()), `${blockType} Markdown should be preserved`);
}

function dispatchReturn() {
  view.dom.dispatchEvent(new window.KeyboardEvent("keydown", {
    key: "Enter",
    code: "Enter",
    keyCode: 13,
    which: 13,
    bubbles: true,
    cancelable: true,
  }));
}

function poll(predicate, delay = 20, attempts = 100) {
  return new Promise((resolve, reject) => {
    const check = () => {
      if (predicate()) return resolve();
      if (--attempts <= 0) return reject(new Error("timeout waiting for editor"));
      setTimeout(check, delay);
    };
    check();
  });
}
