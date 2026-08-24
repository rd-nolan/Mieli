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
ed.focus();
const editorFocusOK = window.document.activeElement?.classList.contains("ProseMirror") === true;
console.log(editorFocusOK ? "PASS  editor focus API" : "FAIL  editor focus API");
if (!editorFocusOK) process.exitCode = 1;

ed.setMarkdown("# Language\n\nKeep **Markdown** unchanged.");
const beforeLanguageChange = ed.getMarkdown();
ed.setLanguage("en");
const englishLanguageOK = window.document.documentElement.lang === "en";
ed.setLanguage("zh-Hans");
const chineseLanguageOK = window.document.documentElement.lang === "zh-Hans";
const languagePreservesMarkdown = ed.getMarkdown() === beforeLanguageChange;
const languageAPIExists = englishLanguageOK && chineseLanguageOK && languagePreservesMarkdown;
console.log(languageAPIExists
  ? "PASS  runtime editor language API"
  : `FAIL  runtime editor language API en=${englishLanguageOK} zh=${chineseLanguageOK} markdown=${languagePreservesMarkdown}`);
if (!languageAPIExists) process.exitCode = 1;

const cases = [
  ["# Hello\n\n**World**", "# Hello\n\n**World**"],
  ["今天研究了 Spring 状态机的实现方案", "今天研究了 Spring 状态机的实现方案"],
  ["- 甲\n- 乙", "- 甲\n- 乙"],
  ["1. First\n2. Second", "1. First\n2. Second"],
  ["> Quoted text", "> Quoted text"],
  ["**bold** *italic* ~~strike~~ `inline`", "**bold** *italic* ~~strike~~ `inline`"],
  ["[Minne](https://example.com)\n\n![Alt](image.png)", "[Minne](https://example.com)\n\n![Alt](image.png)"],
  ["```js\nconst value = 1;\n```", "```js\nconst value = 1;\n```"],
  ["Setext heading\n--------------", "## Setext heading"],
  ["    const indented = true;", "```\nconst indented = true;\n```"],
  ["[Reference][docs]\n\n[docs]: https://example.com \"Docs\"", "[Reference](https://example.com \"Docs\")"],
  ["<https://example.com>", "<https://example.com>"],
  ["\\*literal\\* &amp; entity", "\\*literal\\* & entity"],
  ["hard break  \nnext line", "hard break\\\nnext line"],
  ["<span>inline HTML</span>", "<span>inline HTML</span>"],
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
const renderedTableWrapper = renderedTable?.closest('[data-table-wrapper="true"]');
const taskBoxes = [...(editorRoot?.querySelectorAll('input[type="checkbox"]') ?? [])];
const structureOK = Boolean(renderedTable) && Boolean(renderedTableWrapper) && taskBoxes.length === 2
  && taskBoxes[0].checked && !taskBoxes[1].checked;
console.log(structureOK
  ? "PASS  table and task-list DOM"
  : `FAIL  table/task DOM table=${Boolean(renderedTable)} wrapper=${Boolean(renderedTableWrapper)} boxes=${taskBoxes.length}`);
if (!structureOK) process.exitCode = 1;

// T122: the complete currently-supported CommonMark/GFM surface should render
// semantically and remain stable after a save/reload round-trip. Mermaid and
// LaTeX are intentionally preservation-only until T123/T124 add renderers.
const syntaxMarkdown = [
  "# H1",
  "## H2",
  "### H3",
  "#### H4",
  "##### H5",
  "###### H6",
  "",
  "Setext heading",
  "--------------",
  "",
  "> Quote",
  "",
  "- Bullet",
  "  - Nested",
  "",
  "1. Ordered",
  "2. Second",
  "",
  "- [x] Completed",
  "- [ ] Pending",
  "",
  "**bold** *italic* ~~strike~~ `inline` [link](https://example.com)",
  "",
  "[Reference][docs] and <https://example.org>",
  "",
  "[docs]: https://example.com \"Docs\"",
  "",
  "\\*literal\\* &amp; entity",
  "",
  "hard break  ",
  "next line",
  "",
  "<span data-minne-html=\"inline\">inline HTML</span>",
  "",
  "<div data-minne-html=\"block\">block HTML</div>",
  "",
  "![Alt](image.png)",
  "",
  "---",
  "",
  "```swift",
  "let value = 1",
  "```",
  "",
  "    const indented = true;",
  "",
  "```mermaid",
  "graph TD; A-->B",
  "```",
  "",
  "Inline $x^2$ and block:",
  "",
  "$$",
  "E=mc^2",
  "$$",
  "",
  "| A | B |",
  "| --- | --- |",
  "| 1 | 2 |",
].join("\n");
ed.setMarkdown(syntaxMarkdown);
const firstSyntaxRoundTrip = ed.getMarkdown();
const syntaxRoot = window.document.querySelector(".ProseMirror");
const syntaxDOMOK = ["h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "ul", "ol", "del", "code", "img", "hr", "pre", "table"]
  .every((selector) => syntaxRoot?.querySelector(selector));
const extendedSyntaxChecks = {
  setext: [...(syntaxRoot?.querySelectorAll("h2") ?? [])]
    .some((node) => node.textContent === "Setext heading"),
  indentedCode: [...(syntaxRoot?.querySelectorAll("pre") ?? [])]
    .some((node) => node.textContent.includes("const indented = true;")),
  referenceLink: Boolean(syntaxRoot?.querySelector('a[href="https://example.com"]')),
  autolink: Boolean(syntaxRoot?.querySelector('a[href="https://example.org"]')),
  hardBreak: Boolean(syntaxRoot?.querySelector("br")),
  inlineHTML: firstSyntaxRoundTrip.includes('<span data-minne-html="inline">inline HTML</span>'),
  blockHTML: firstSyntaxRoundTrip.includes('<div data-minne-html="block">block HTML</div>'),
};
const extendedSyntaxDOMOK = Object.values(extendedSyntaxChecks).every(Boolean);
const mermaidIsPreservedCode = Boolean(syntaxRoot?.querySelector('pre[data-language="mermaid"]'))
  && !syntaxRoot?.querySelector("svg");
const latexIsPreservedText = firstSyntaxRoundTrip.includes("$x^2$")
  && firstSyntaxRoundTrip.includes("$$\nE=mc^2\n$$");
ed.setMarkdown(firstSyntaxRoundTrip);
const syntaxRoundTripStable = ed.getMarkdown() === firstSyntaxRoundTrip;
const completeSyntaxOK = syntaxDOMOK && extendedSyntaxDOMOK && mermaidIsPreservedCode
  && latexIsPreservedText && syntaxRoundTripStable;
console.log(completeSyntaxOK
  ? "PASS  complete CommonMark/GFM DOM and round-trip"
  : `FAIL  complete syntax DOM=${syntaxDOMOK} extended=${JSON.stringify(extendedSyntaxChecks)} mermaid=${mermaidIsPreservedCode} latex=${latexIsPreservedText} stable=${syntaxRoundTripStable}`);
if (!completeSyntaxOK) process.exitCode = 1;

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

// T117: a semantic code block must also have an immediately recognizable
// visual container. Check computed behavior rather than the CSS source text.
ed.setMarkdown("```swift\nlet value = 1\n```");
const renderedCodeBlock = editorRoot?.querySelector("pre");
const codeBlockStyle = renderedCodeBlock
  ? window.getComputedStyle(renderedCodeBlock)
  : null;
const transparentColors = new Set(["", "transparent", "rgba(0, 0, 0, 0)"]);
const codeBlockStyleOK = Boolean(codeBlockStyle)
  && !transparentColors.has(codeBlockStyle.backgroundColor)
  && Number.parseFloat(codeBlockStyle.paddingTop) > 0
  && Number.parseFloat(codeBlockStyle.borderRadius) > 0
  && codeBlockStyle.fontFamily.toLowerCase().includes("mono");
console.log(codeBlockStyleOK
  ? "PASS  code block visual container"
  : `FAIL  code block style background=${codeBlockStyle?.backgroundColor} padding=${codeBlockStyle?.paddingTop} radius=${codeBlockStyle?.borderRadius} font=${codeBlockStyle?.fontFamily}`);
if (!codeBlockStyleOK) process.exitCode = 1;

const codeBlockLabelRule = /\.ProseMirror\s+pre::before\s*\{([^}]*)\}/s
  .exec(html)?.[1] ?? "";
const sharedBlockLabelRule = /\.ProseMirror\s*>\s*h1::before,[\s\S]*?\.ProseMirror\s*>\s*hr::before\s*\{([^}]*)\}/s
  .exec(html)?.[1] ?? "";
const codeBlockLabelStyleOK = /content:\s*var\(--minne-label-code\)/.test(codeBlockLabelRule)
  && /writing-mode:\s*vertical-rl/.test(sharedBlockLabelRule)
  && /right:\s*calc\(100%\s*\+/.test(sharedBlockLabelRule);
console.log(codeBlockLabelStyleOK
  ? "PASS  outside vertical code block label"
  : "FAIL  outside vertical code block label");
if (!codeBlockLabelStyleOK) process.exitCode = 1;

const verticalBlockLabels = [
  ["h1", "[\\\"']H1[\\\"']"],
  ["h2", "[\\\"']H2[\\\"']"],
  ["h3", "[\\\"']H3[\\\"']"],
  ["h4", "[\\\"']H4[\\\"']"],
  ["h5", "[\\\"']H5[\\\"']"],
  ["h6", "[\\\"']H6[\\\"']"],
  ["blockquote", "var\\(--minne-label-quote\\)"],
  ["ul", "var\\(--minne-label-unordered-list\\)"],
  ["ol", "var\\(--minne-label-ordered-list\\)"],
  ["ul:has(> li[data-task-item])", "var\\(--minne-label-task-list\\)"],
  ['[data-table-wrapper="true"]', "var\\(--minne-label-table\\)"],
  ["hr", "var\\(--minne-label-divider\\)"],
];
const missingBlockLabels = verticalBlockLabels.filter(([selector, content]) => {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const rules = [...html.matchAll(
    new RegExp(`\\.ProseMirror\\s*>\\s*${escapedSelector}::before\\s*\\{([^}]*)\\}`, "gs"),
  )].map((match) => match[1]);
  return !rules.some((rule) => new RegExp(`content:\\s*${content}`).test(rule));
});
const mermaidLabelOK = /\.ProseMirror\s+pre\[data-language=["']mermaid["']\]::before\s*\{[^}]*content:\s*["']Mermaid["']/s
  .test(html);
const verticalLabelLayoutOK = /writing-mode:\s*vertical-rl/.test(sharedBlockLabelRule)
  && /white-space:\s*nowrap/.test(sharedBlockLabelRule)
  && /right:\s*calc\(100%\s*\+/.test(sharedBlockLabelRule);
const headingLabelRules = [...html.matchAll(
  /\.ProseMirror\s*>\s*h1::before,[\s\S]*?\.ProseMirror\s*>\s*h6::before\s*\{([^}]*)\}/gs,
)].map((match) => match[1]);
const headingLabelCombinationOK = headingLabelRules.some((rule) =>
  /text-combine-upright:\s*all/.test(rule)
    && /writing-mode:\s*horizontal-tb/.test(rule)
    && /letter-spacing:\s*0/.test(rule));
console.log(missingBlockLabels.length === 0 && verticalLabelLayoutOK && mermaidLabelOK
  ? "PASS  vertical labels for all block types"
  : `FAIL  vertical block labels missing=${missingBlockLabels.map(([s]) => s).join(",")} layout=${verticalLabelLayoutOK} mermaid=${mermaidLabelOK}`);
if (missingBlockLabels.length || !verticalLabelLayoutOK || !mermaidLabelOK) process.exitCode = 1;
console.log(headingLabelCombinationOK
  ? "PASS  combined upright heading labels"
  : "FAIL  combined upright heading labels");
if (!headingLabelCombinationOK) process.exitCode = 1;

const editorLanguageVariablesOK = /:root\s*\{[^}]*--minne-label-code:\s*["']Code["'][^}]*--minne-hint-code-exit:\s*["']⌘ Return to exit["']/s.test(html)
  && /:root:lang\(zh-Hans\)[^{]*\{[^}]*--minne-label-code:\s*["']代码["'][^}]*--minne-hint-code-exit:\s*["']⌘ Return 跳出["']/s.test(html);
const codeBlockExitHintOK = /\.ProseMirror\s+pre::after\s*\{[^}]*content:\s*var\(--minne-hint-code-exit\)/s.test(html)
  && editorLanguageVariablesOK;
console.log(codeBlockExitHintOK
  ? "PASS  code block Command-Return exit hint"
  : "FAIL  code block Command-Return exit hint");
if (!codeBlockExitHintOK) process.exitCode = 1;

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
