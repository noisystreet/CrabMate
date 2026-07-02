/**
 * CodeMirror 6 IDE 编辑器桥接（由 esbuild 打包为 `vendor/ide-codemirror.js`）。
 * Rust 侧通过 `globalThis.CrabMateIdeEditor` 调用。
 */
import { defaultKeymap, history, historyKeymap, indentLess, insertTab } from "@codemirror/commands";
import { cpp } from "@codemirror/lang-cpp";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { yaml } from "@codemirror/lang-yaml";
import {
  bracketMatching,
  defaultHighlightStyle,
  foldGutter,
  indentOnInput,
  indentUnit,
  syntaxHighlighting,
} from "@codemirror/language";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState } from "@codemirror/state";
import {
  crosshairCursor,
  drawSelection,
  dropCursor,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";

/** @type {Map<number, { view: EditorView, lang: Compartment, readOnly: Compartment, lineNums: Compartment, wrap: Compartment, theme: Compartment, tabSize: Compartment }>} */
const editors = new Map();
let nextId = 1;

/**
 * @param {string | undefined} langId
 * @returns {import('@codemirror/state').Extension}
 */
function langExtension(langId) {
  switch (langId) {
    case "rust":
      return rust();
    case "toml":
    case "json":
      return json();
    case "yaml":
      return yaml();
    case "c":
    case "cpp":
      return cpp();
    case "python":
      return python();
    case "javascript":
    case "typescript":
      return javascript({ typescript: langId === "typescript" });
    case "markdown":
      return markdown();
    case "shell":
    case "go":
    default:
      return [];
  }
}

/**
 * @param {object} options
 * @param {(id: number, text: string) => void} onChange
 */
function buildExtensions(options, onChange) {
  const id = options.id;
  const langComp = new Compartment();
  const readOnlyComp = new Compartment();
  const lineNumsComp = new Compartment();
  const wrapComp = new Compartment();
  const themeComp = new Compartment();
  const tabSizeComp = new Compartment();

  const tabSize = Math.max(1, Math.min(8, options.tabSize || 4));
  const fontSize = options.fontSize || 14;
  const fontFamily =
    options.fontFamily ||
    '"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace';

  const base = [
    highlightSpecialChars(),
    history(),
    foldGutter(),
    drawSelection(),
    dropCursor(),
    EditorState.allowMultipleSelections.of(true),
    indentOnInput(),
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    bracketMatching(),
    rectangularSelection(),
    crosshairCursor(),
    highlightActiveLine(),
    highlightActiveLineGutter(),
    highlightSelectionMatches(),
    keymap.of([
      ...defaultKeymap,
      ...historyKeymap,
      ...searchKeymap,
      // insertTab：无选区时在光标处插入 tab 宽度空格；有选区时缩进行（indentWithTab 总在行首缩进）
      { key: "Tab", run: insertTab, shift: indentLess },
    ]),
    EditorView.updateListener.of((update) => {
      if (update.docChanged && onChange) {
        onChange(id, update.state.doc.toString());
      }
    }),
    langComp.of(langExtension(options.lang)),
    readOnlyComp.of(EditorState.readOnly.of(!!options.readOnly)),
    lineNumsComp.of(options.lineNumbers ? lineNumbers() : []),
    wrapComp.of(options.wordWrap ? EditorView.lineWrapping : []),
    tabSizeComp.of(indentUnit.of(" ".repeat(tabSize))),
    themeComp.of(
      EditorView.theme({
        "&": {
          height: "100%",
          fontSize: `${fontSize}px`,
          fontFamily,
        },
        "&.cm-focused": {
          outline: "none",
        },
        ".cm-scroller": {
          fontFamily: "inherit",
          lineHeight: "1.45",
        },
        ".cm-gutters": {
          fontFamily: "inherit",
          fontSize: "inherit",
        },
      }),
    ),
  ];

  return { extensions: base, langComp, readOnlyComp, lineNumsComp, wrapComp, themeComp, tabSizeComp };
}

/**
 * @param {HTMLElement} parent
 * @param {object} options
 * @param {(id: number, text: string) => void} onChange
 * @returns {number}
 */
function create(parent, options, onChange) {
  parent.replaceChildren();
  const id = nextId++;
  const opts = { ...options, id };
  const built = buildExtensions(opts, onChange);
  const state = EditorState.create({
    doc: options.doc || "",
    extensions: built.extensions,
  });
  const view = new EditorView({ state, parent });
  editors.set(id, {
    view,
    lang: built.langComp,
    readOnly: built.readOnlyComp,
    lineNums: built.lineNumsComp,
    wrap: built.wrapComp,
    theme: built.themeComp,
    tabSize: built.tabSizeComp,
  });
  return id;
}

/**
 * @param {number} id
 */
function destroy(id) {
  const rec = editors.get(id);
  if (!rec) return;
  rec.view.destroy();
  editors.delete(id);
}

/**
 * @param {number} id
 * @returns {string}
 */
function getDoc(id) {
  const rec = editors.get(id);
  return rec ? rec.view.state.doc.toString() : "";
}

/**
 * @param {number} id
 * @param {string} text
 */
function setDoc(id, text) {
  const rec = editors.get(id);
  if (!rec) return;
  const cur = rec.view.state.doc.toString();
  if (cur === text) return;
  rec.view.dispatch({
    changes: { from: 0, to: rec.view.state.doc.length, insert: text },
  });
}

/**
 * @param {number} id
 * @param {object} patch
 */
function reconfigure(id, patch) {
  const rec = editors.get(id);
  if (!rec) return;
  const effects = [];
  if (patch.lang !== undefined) {
    effects.push(rec.lang.reconfigure(langExtension(patch.lang)));
  }
  if (patch.readOnly !== undefined) {
    effects.push(rec.readOnly.reconfigure(EditorState.readOnly.of(!!patch.readOnly)));
  }
  if (patch.lineNumbers !== undefined) {
    effects.push(rec.lineNums.reconfigure(patch.lineNumbers ? lineNumbers() : []));
  }
  if (patch.wordWrap !== undefined) {
    effects.push(rec.wrap.reconfigure(patch.wordWrap ? EditorView.lineWrapping : []));
  }
  if (patch.tabSize !== undefined) {
    const n = Math.max(1, Math.min(8, patch.tabSize || 4));
    effects.push(rec.tabSize.reconfigure(indentUnit.of(" ".repeat(n))));
  }
  if (patch.fontSize !== undefined || patch.fontFamily !== undefined) {
    const fontSize = patch.fontSize || 14;
    const fontFamily =
      patch.fontFamily ||
      '"JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace';
    effects.push(
      rec.theme.reconfigure(
        EditorView.theme({
          "&": {
            height: "100%",
            fontSize: `${fontSize}px`,
            fontFamily,
          },
          "&.cm-focused": { outline: "none" },
          ".cm-scroller": { fontFamily: "inherit", lineHeight: "1.45" },
          ".cm-gutters": { fontFamily: "inherit", fontSize: "inherit" },
        }),
      ),
    );
  }
  if (effects.length) {
    rec.view.dispatch({ effects });
  }
}

/**
 * @param {number} id
 */
function focus(id) {
  const rec = editors.get(id);
  if (rec) rec.view.focus();
}

/**
 * 容器从隐藏变为可见或布局变化后，强制 CM 重新测量尺寸（WebKit / Tauri 常见空白）。
 * @param {number} id
 */
function requestMeasure(id) {
  const rec = editors.get(id);
  if (!rec) return;
  rec.view.requestMeasure();
}

/**
 * @param {number} id
 */
function selectAll(id) {
  const rec = editors.get(id);
  if (!rec) return;
  rec.view.dispatch({
    selection: { anchor: 0, head: rec.view.state.doc.length },
  });
  rec.view.focus();
}

/**
 * UTF-16 offset for char index (BMP-safe; matches existing find helpers).
 * @param {string} text
 * @param {number} charIdx
 */
function charIndexToUtf16(text, charIdx) {
  let u16 = 0;
  let chars = 0;
  for (const ch of text) {
    if (chars >= charIdx) break;
    u16 += ch.length;
    chars += 1;
  }
  return u16;
}

/**
 * @param {number} id
 * @param {number} startChar
 * @param {number} endChar
 */
function setSelectionChars(id, startChar, endChar) {
  const rec = editors.get(id);
  if (!rec) return;
  const text = rec.view.state.doc.toString();
  const anchor = charIndexToUtf16(text, startChar);
  const head = charIndexToUtf16(text, endChar);
  rec.view.dispatch({
    selection: { anchor, head },
    scrollIntoView: true,
  });
  rec.view.focus();
}

/**
 * @param {number} id
 * @param {number} lineOneBased
 */
function gotoLine(id, lineOneBased) {
  const rec = editors.get(id);
  if (!rec) return;
  const doc = rec.view.state.doc;
  const total = doc.lines;
  const line = Math.max(1, Math.min(lineOneBased, total));
  const pos = doc.line(line).from;
  rec.view.dispatch({
    selection: { anchor: pos, head: pos },
    scrollIntoView: true,
  });
  rec.view.focus();
}

export {
  create,
  destroy,
  focus,
  getDoc,
  gotoLine,
  reconfigure,
  requestMeasure,
  selectAll,
  setDoc,
  setSelectionChars,
};
