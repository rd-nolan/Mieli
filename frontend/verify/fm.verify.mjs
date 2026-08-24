import { JSDOM } from "jsdom";
import { readFileSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const res = path.resolve(__dirname, "../../Muisti/Resources");
const html = readFileSync(path.join(res, "editor.html"), "utf8");
const js = readFileSync(path.join(res, "editor.bundle.js"), "utf8");
const dom = new JSDOM(html, { url: "file:///muisti/editor.html", runScripts: "dangerously", beforeParse(w){ w.console=console; } });
const { window } = dom;
window.webkit = { messageHandlers: { editorContent: { postMessage(){} } } };
const script = window.document.createElement("script");
script.textContent = js;
window.document.body.appendChild(script);
function poll(fn, ms=60, tries=200){ return new Promise((res,rej)=>{ const step=()=>{ const v=fn(); if(v) return res(v); if(--tries<=0) return rej(new Error("timeout")); setTimeout(step,ms); }; step(); }); }
await poll(()=>window.muistiEditor?.isReady());
const ed = window.muistiEditor;
let fail = 0;
const chk = (cond, msg) => { console.log((cond?"PASS ":"FAIL ")+msg); if(!cond) fail++; };

const FM = "---\nid: AL2026fm\ntags:\n  - Swift\ncreated: 2026-08-23T08:00:00+08:00\nupdated: 2026-08-23T08:30:00+08:00\n---";
ed.setMarkdown(FM + "\n# 可见标题\n\n正文内容");
let rendered = window.document.querySelector('.ProseMirror').textContent;
chk(!rendered.includes("AL2026fm") && !rendered.includes("created") && !rendered.includes("updated"), "渲染不显示 front matter");
const out = ed.getMarkdown();
chk(out.includes("id: AL2026fm"), "roundtrip 含 id");
chk(out.includes("---\nid"), "保留 --- 框架");
chk(out.includes("# 可见标题"), "roundtrip 含正文");
chk(!out.includes("***"), "front matter 未被损坏成 ***");
ed.setMarkdown("# 普通\n\n内容");
chk(ed.getMarkdown().trim() === "# 普通\n\n内容", "无 front matter 原样透传");
ed.setMarkdown(FM + "\n# 标题\n\n正文");
chk(ed.getMarkdown().includes("AL2026fm"), "编辑后仍含 id");

// 损坏的 front matter（历史 bug 把 --- 变成 ***…long-dashes）也必须剥离显示。
// 见 AGENTS §9：id 是稳定标识，只能隐藏、绝不能丢失。
const CORRUPT = "***\n\nid: AL2corrupt\ntags: \\[]\ncreated: T\nupdated: T\n-----------------------------\n\n#### 正文标题\n";
ed.setMarkdown(CORRUPT);
let rendered2 = window.document.querySelector('.ProseMirror').textContent;
chk(!rendered2.includes("AL2corrupt") && !rendered2.includes("created"), "损坏格式 front matter 不渲染");
const out2 = ed.getMarkdown();
chk(out2.includes("AL2corrupt"), "损坏格式 roundtrip 保留 id");
chk(!out2.includes("***") || out2.includes("AL2corrupt"), "损坏格式保存仍含数据");
// 真实生成的普通横线不误判为 front matter
ed.setMarkdown("---\n\n# 标题\n\n正文");
chk(window.document.querySelector('.ProseMirror').textContent.includes("标题"), "普通横线保留渲染");

process.exitCode = fail ? 1 : 0;
console.log(fail ? "FM VERIFY FAILED" : "FM VERIFY OK");
