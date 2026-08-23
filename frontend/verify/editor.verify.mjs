// Verification (T061): confirm the Milkdown bundle boots in a DOM and that
// Markdown→editor→Markdown round-trips. Run: node verify/editor.verify.mjs
import { JSDOM } from "jsdom";
import { readFileSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const res = path.resolve(__dirname, "../../Minne/Resources");

const html = readFileSync(path.join(res, "editor.html"), "utf8");
const js = readFileSync(path.join(res, "editor.bundle.js"), "utf8");

// Frame the host page with the bundle inlined, plus globals Milkdown expects.
const dom = new JSDOM(html, {
  url: "file:///minne/editor.html",
  runScripts: "dangerously",
  beforeParse(window) {
    window.console = console;
  },
});
const { window } = dom;

// Mock the WKWebView message bridge. Editor-driven notifications post here;
// T063 guard means programmatic loads must NOT notify (else save-load self-spam).
const bridgeMessages = [];
window.webkit = {
  messageHandlers: {
    editorContent: {
      postMessage(msg) {
        bridgeMessages.push(msg);
      },
    },
  },
};

// Inline the bundle as a script element (the <script src> tag won't resolve in jsdom).
const script = window.document.createElement("script");
script.textContent = js;
window.document.body.appendChild(script);

// Wait for minneEditor to appear (boot() is async).
function poll(fn, ms = 60, tries = 100) {
  return new Promise((resolve, reject) => {
    const step = () => {
      const v = fn();
      if (v) return resolve(v);
      if (--tries <= 0) return reject(new Error("timeout waiting for editor"));
      setTimeout(step, ms);
    };
    step();
  });
}

await poll(() => window.minneEditor && window.minneEditor.isReady());

const ed = window.minneEditor;
const cases = [
  ["# Hello\n\n**World**", "# Hello\n\n**World**"],
  ["今天研究了 Spring 状态机的实现方案", "今天研究了 Spring 状态机的实现方案"],
  ["- 甲\n- 乙", "- 甲\n- 乙"],
  [
    "| 列一 | 列二 |\n| --- | --- |\n| 中文 | English |\n| 123 | 456 |",
    "| 列一  | 列二      |\n| --- | ------- |\n| 中文  | English |\n| 123 | 456     |",
  ],
  ["- [x] 已完成\n- [ ] 未完成", "- [x] 已完成\n- [ ] 未完成"],
];

let fail = 0;
for (const [md, expect] of cases) {
  ed.setMarkdown(md);
  const out = ed.getMarkdown();
  const norm = (s) => s.trim();
  const ok = norm(out) === norm(expect);
  console.log(`${ok ? "PASS" : "FAIL"}  in=(${JSON.stringify(md)})  out=(${JSON.stringify(out)})`);
  if (!ok) fail++;
}

process.exitCode = fail ? 1 : 0;
console.log(fail ? "VERIFY FAILED" : "VERIFY OK");

// T114: GFM structures must render as semantic editor nodes rather than
// failing the whole document or degrading task items to literal text.
ed.setMarkdown("| A | B |\n| --- | --- |\n| 1 | 2 |\n\n- [x] Done\n- [ ] Todo");
const editorRoot = window.document.querySelector(".ProseMirror");
const renderedTable = editorRoot?.querySelector("table");
const taskBoxes = [...(editorRoot?.querySelectorAll('input[type="checkbox"]') ?? [])];
const structureOK = Boolean(renderedTable) && taskBoxes.length === 2
  && taskBoxes[0].checked && !taskBoxes[1].checked;
console.log(structureOK
  ? "PASS  table and task-list DOM"
  : `FAIL  table/task DOM table=${Boolean(renderedTable)} boxes=${taskBoxes.length}`);
if (!structureOK) process.exitCode = 1;

const requiredStyles = [
  ".ProseMirror ul",
  ".ProseMirror ol",
  ".ProseMirror blockquote",
  ".ProseMirror :not(pre) > code",
  ".ProseMirror table",
];
const missingStyles = requiredStyles.filter((selector) => !html.includes(selector));
console.log(missingStyles.length === 0
  ? "PASS  Markdown presentation styles"
  : `FAIL  missing styles: ${missingStyles.join(", ")}`);
if (missingStyles.length) process.exitCode = 1;

// T063: programmatic loads (setMarkdown) must not emit contentChanged —
// otherwise a Swift save→reload loop would self-notify forever.
ed.setMarkdown("# 另一个\n\n- 丙\n- 丁");
const changeEvents = bridgeMessages.filter((m) => m.type === "contentChanged");
console.log(changeEvents.length === 0
  ? "PASS  setMarkdown suppresses contentChanged"
  : `FAIL  setMarkdown emitted ${changeEvents.length} contentChanged`);
if (changeEvents.length !== 0) process.exitCode = 1;

// T083: insertAttachment injects a Markdown fragment that survives round-trip.
ed.setMarkdown("# 标题\n\n正文");
ed.insertAttachment("![image](a.files/image.png)");
const withImg = ed.getMarkdown();
const hasImg = withImg.includes("image.png");
console.log(hasImg ? "PASS  insertAttachment" : `FAIL  insertAttachment out=(${JSON.stringify(withImg)})`);
if (!hasImg) process.exitCode = 1;

// T085: relative image srcs resolve against the note folder base (pure).
ed.setImageBaseURI("file:///ws/note/");
const resolveCases = [
  ["./a.files/pic.png", "file:///ws/note/a.files/pic.png"],
  ["a.files/pic.png", "file:///ws/note/a.files/pic.png"],
  ["https://x/y.png", "https://x/y.png"], // already absolute → untouched
  ["/abs/p.png", "/abs/p.png"],
];
let imgFail = 0;
for (const [raw, want] of resolveCases) {
  const got = ed.resolveImageSrc(raw);
  const ok = got === want;
  console.log(`${ok ? "PASS" : "FAIL"}  resolveImageSrc(${raw}) = ${got}`);
  if (!ok) imgFail++;
}
if (imgFail) process.exitCode = 1;
console.log(imgFail ? "VERIFY FAILED (image resolve)" : "VERIFY OK (image resolve)");
