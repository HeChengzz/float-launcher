import "./style.css";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ============================================================================
//  全局关闭右键菜单
// ============================================================================
document.addEventListener(
  "contextmenu",
  (e) => { e.preventDefault(); e.stopPropagation(); return false; },
  true
);

// ============================================================================
//  关闭浏览器表单提示
// ============================================================================
document.addEventListener("DOMContentLoaded", () => {
  document.querySelectorAll("input, textarea").forEach((el) => {
    el.setAttribute("autocomplete", "off");
    el.setAttribute("spellcheck", "false");
    el.setAttribute("autocorrect", "off");
    el.setAttribute("autocapitalize", "off");
    el.setAttribute("data-form-type", "other");
    el.setAttribute("data-lpignore", "true");
    el.setAttribute("data-1p-ignore", "true");
  });
});

// ========== 面板识别 ==========
const params = new URLSearchParams(location.search);
const PANEL = params.get("panel") || "launcher";
const IS_LAUNCHER = PANEL === "launcher";

// ========== DOM ==========
const titleEl   = document.getElementById("title");
const listEl    = document.getElementById("list");
const viewBtn   = document.getElementById("btn-view");
const scopeBtn  = document.getElementById("btn-scope");
const settingsBtn = document.getElementById("btn-settings");
const btnFolder = document.getElementById("btn-new-folder");
const btnFile   = document.getElementById("btn-new-file");

const searchWrap  = document.getElementById("search-wrap");
const searchInput = document.getElementById("search-input");
const searchClear = document.getElementById("search-clear");

const confirmOverlay = document.getElementById("confirm-overlay");
const confirmTitleEl = document.getElementById("confirm-title");
const confirmMsgEl   = document.getElementById("confirm-msg");
const confirmOkBtn   = document.getElementById("confirm-ok");
const confirmCancelBtn = document.getElementById("confirm-cancel");

const inputOverlay = document.getElementById("input-overlay");
const inputTitleEl = document.getElementById("input-title");
const inputField   = document.getElementById("input-field");
const inputOkBtn   = document.getElementById("input-ok");
const inputCancelBtn = document.getElementById("input-cancel");

// ========== 图标 ==========
const ICON_GRID = `
  <svg class="icon" viewBox="0 0 16 16" fill="none"
       stroke="currentColor" stroke-width="1.5"
       stroke-linecap="round" stroke-linejoin="round">
    <rect x="2.75" y="2.75" width="4.25" height="4.25" rx="1"/>
    <rect x="9"    y="2.75" width="4.25" height="4.25" rx="1"/>
    <rect x="2.75" y="9"    width="4.25" height="4.25" rx="1"/>
    <rect x="9"    y="9"    width="4.25" height="4.25" rx="1"/>
  </svg>`;

const ICON_LIST = `
  <svg class="icon" viewBox="0 0 16 16" fill="none"
       stroke="currentColor" stroke-width="1.5"
       stroke-linecap="round" stroke-linejoin="round">
    <circle cx="3.25" cy="4"  r="0.85" fill="currentColor" stroke="none"/>
    <circle cx="3.25" cy="8"  r="0.85" fill="currentColor" stroke="none"/>
    <circle cx="3.25" cy="12" r="0.85" fill="currentColor" stroke="none"/>
    <path d="M6.5 4h7M6.5 8h7M6.5 12h7"/>
  </svg>`;

const ICON_DESKTOP_ONLY = `
  <svg class="icon" viewBox="0 0 16 16" fill="none"
       stroke="currentColor" stroke-width="1.5"
       stroke-linecap="round" stroke-linejoin="round">
    <rect x="2.5" y="3" width="11" height="9.5" rx="1.5"/>
    <path d="M2.5 5.75h11"/>
    <path d="M5.25 4.4h0.01M6.9 4.4h0.01"/>
  </svg>`;

const ICON_SHOW_ALL = `
  <svg class="icon" viewBox="0 0 16 16" fill="none"
       stroke="currentColor" stroke-width="1.5"
       stroke-linecap="round" stroke-linejoin="round">
    <rect x="5.75" y="2.5" width="7.75" height="7.75" rx="1.5"/>
    <path d="M10.25 10.25v.5a1.5 1.5 0 0 1-1.5 1.5H3.5a1.5 1.5 0 0 1-1.5-1.5V6a1.5 1.5 0 0 1 1.5-1.5h.5"/>
  </svg>`;

const ICON_DELETE = `
  <svg viewBox="0 0 16 16" fill="none"
       stroke="currentColor" stroke-width="2"
       stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 4l8 8M12 4l-8 8"/>
  </svg>`;

const ICON_RENAME = `
  <svg viewBox="0 0 16 16" fill="none"
       stroke="currentColor" stroke-width="1.6"
       stroke-linecap="round" stroke-linejoin="round">
    <path d="M11.2 2.3l2.5 2.5-8.4 8.4-3.1 0.6 0.6-3.1z"/>
    <path d="M9.7 3.8l2.5 2.5"/>
  </svg>`;

// ========== 状态 ==========
const LS_KEY = `panel.${PANEL}`;
let items = [];
let currentVisible = [];
let selected = -1;
let viewMode = localStorage.getItem(`${LS_KEY}.view`) || "grid";
let showAll = localStorage.getItem(`${LS_KEY}.showAll`) === "1";

let iconStyle = "mask";
let settings = null;

let currentQuery = "";

let allItemsCache = null;
let allItemsPromise = null;
let renderSeq = 0;

// ========== 内置字体映射 ==========
const FONT_MAP = {
  segoe:  '"Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif',
  yahei:  '"Microsoft YaHei UI", "Microsoft YaHei", sans-serif',
  mono:   '"Cascadia Mono", "Consolas", "Courier New", monospace',
  system: 'system-ui, -apple-system, "Segoe UI", sans-serif',
};

// ============================================================================
//  主题
// ============================================================================
const systemLightMQ = window.matchMedia("(prefers-color-scheme: light)");

function resolveTheme(theme) {
  if (theme === "light" || theme === "dark") return theme;
  return systemLightMQ.matches ? "light" : "dark";
}

function applyTheme(theme) {
  const actual = resolveTheme(theme);
  document.documentElement.setAttribute("data-theme", actual);
  updateBgAlpha();
}

systemLightMQ.addEventListener("change", () => {
  if (!settings || settings.theme === "system") {
    applyTheme(settings?.theme || "system");
  }
});

// ============================================================================
//  ★ 计算面板背景色 + 整体不透明度
// ============================================================================
function updateBgAlpha() {
  const theme = document.documentElement.getAttribute("data-theme") || "dark";
  const effect = document.documentElement.getAttribute("data-effect") || "none";
  const ms = (settings?.mica_strength ?? 30) / 100;
  const opacity = (settings?.opacity ?? 100) / 100;   // 0~1

  let alpha;
  if (effect === "none") {
    alpha = 1;
  } else if (effect === "acrylic") {
    alpha = theme === "light"
      ? (0.75 + 0.25 * ms)
      : (0.15 + 0.85 * ms);
  } else {
    alpha = theme === "light"
      ? (0.70 + 0.30 * ms)
      : ms;
  }

  // ★ 关键：把"面板透明度"乘进背景 alpha，而不是改元素 opacity
  alpha *= opacity;

  const r = theme === "light" ? 248 : 32;
  const g = theme === "light" ? 249 : 32;
  const b = theme === "light" ? 252 : 32;
  const r2 = theme === "light" ? 232 : 20;
  const g2 = theme === "light" ? 234 : 20;
  const b2 = theme === "light" ? 240 : 20;

  const root = document.documentElement;
  root.style.setProperty("--panel-tint-1", `rgba(${r}, ${g}, ${b}, ${alpha})`);
  root.style.setProperty("--panel-tint-2", `rgba(${r2}, ${g2}, ${b2}, ${alpha * 0.85})`);

  console.log(
    `[bg] theme=${theme} effect=${effect} mica=${settings?.mica_strength} opacity=${settings?.opacity} → alpha=${alpha.toFixed(3)}`
  );
}
// ============================================================================
//  系统材质
// ============================================================================
async function loadSystemInfo() {
  try {
    const info = await invoke("get_system_info");
    document.documentElement.setAttribute("data-effect", info.effect || "none");
    updateBgAlpha();
  } catch (e) {
    console.warn("get_system_info failed:", e);
    document.documentElement.setAttribute("data-effect", "none");
    updateBgAlpha();
  }
}

listen("system-effect-changed", (e) => {
  document.documentElement.setAttribute("data-effect", e.payload || "none");
  updateBgAlpha();
});

// ============================================================================
//  自定义确认弹窗
// ============================================================================
let confirmResolve = null;

function showConfirm(title, message, okText = "确定") {
  return new Promise((resolve) => {
    confirmTitleEl.textContent = title;
    confirmMsgEl.textContent = message;
    confirmOkBtn.textContent = okText;
    confirmOverlay.classList.add("show");
    confirmResolve = resolve;
    setTimeout(() => confirmCancelBtn.focus(), 30);
  });
}

function closeConfirm(result) {
  confirmOverlay.classList.remove("show");
  if (confirmResolve) {
    confirmResolve(result);
    confirmResolve = null;
  }
}

confirmOkBtn.addEventListener("click", () => closeConfirm(true));
confirmCancelBtn.addEventListener("click", () => closeConfirm(false));
confirmOverlay.addEventListener("click", (e) => {
  if (e.target === confirmOverlay) closeConfirm(false);
});

confirmOverlay.addEventListener("keydown", (e) => {
  e.stopPropagation();
  if (e.key === "Escape") {
    e.preventDefault();
    closeConfirm(false);
  } else if (e.key === "Enter") {
    e.preventDefault();
    if (document.activeElement === confirmCancelBtn) {
      closeConfirm(false);
    } else {
      closeConfirm(true);
    }
  }
});

// ============================================================================
//  自定义输入弹窗
// ============================================================================
let inputResolve = null;

function showInput(title, defaultValue = "") {
  return new Promise((resolve) => {
    inputTitleEl.textContent = title;
    inputField.value = defaultValue;
    inputOverlay.classList.add("show");
    inputResolve = resolve;
    setTimeout(() => {
      inputField.focus();
      inputField.select();
    }, 30);
  });
}

function closeInput(result) {
  inputOverlay.classList.remove("show");
  if (inputResolve) {
    inputResolve(result);
    inputResolve = null;
  }
}

inputOkBtn.addEventListener("click", () => {
  const v = inputField.value.trim();
  closeInput(v ? v : null);
});
inputCancelBtn.addEventListener("click", () => closeInput(null));
inputOverlay.addEventListener("click", (e) => {
  if (e.target === inputOverlay) closeInput(null);
});

inputOverlay.addEventListener("keydown", (e) => {
  e.stopPropagation();
  if (e.key === "Enter") {
    e.preventDefault();
    const v = inputField.value.trim();
    closeInput(v ? v : null);
  } else if (e.key === "Escape") {
    e.preventDefault();
    closeInput(null);
  }
});

// ============================================================================
//  自定义字体
// ============================================================================
const CUSTOM_FONT_STYLE_ID = "float-launcher-custom-fonts";
const injectedFonts = new Set();

const FONT_UNICODE_RANGE = [
  "U+0000-024F", "U+0250-02AF", "U+02B0-02FF", "U+0300-036F",
  "U+0370-03FF", "U+0400-04FF", "U+1E00-1EFF", "U+2000-206F",
  "U+2070-209F", "U+20A0-20CF", "U+2100-214F", "U+2150-218F",
  "U+2190-21FF", "U+2200-22FF", "U+2460-24FF", "U+2500-257F",
  "U+25A0-25FF", "U+2600-26FF", "U+2E80-2EFF", "U+2F00-2FDF",
  "U+3000-303F", "U+3040-309F", "U+30A0-30FF", "U+3100-312F",
  "U+3200-32FF", "U+3400-4DBF", "U+4E00-9FFF", "U+F900-FAFF",
  "U+FE30-FE4F", "U+FF00-FFEF", "U+1F300-1F5FF", "U+1F600-1F64F",
  "U+1F900-1F9FF", "U+20000-2A6DF",
].join(", ");

function injectCustomFont(name, path) {
  if (injectedFonts.has(name)) return;
  const url = convertFileSrc(path);
  let styleEl = document.getElementById(CUSTOM_FONT_STYLE_ID);
  if (!styleEl) {
    styleEl = document.createElement("style");
    styleEl.id = CUSTOM_FONT_STYLE_ID;
    document.head.appendChild(styleEl);
  }
  const ext = (path.split(".").pop() || "").toLowerCase();
  const format =
    ext === "otf"   ? "opentype" :
    ext === "woff2" ? "woff2" :
    ext === "woff"  ? "woff" :
                      "truetype";
  const rule = `@font-face {
  font-family: "${name}";
  src: url("${url}") format("${format}");
  font-weight: 100 900;
  font-style: normal;
  font-display: swap;
  unicode-range: ${FONT_UNICODE_RANGE};
}
`;
  styleEl.textContent += rule;
  injectedFonts.add(name);
}

function rebuildCustomFontStyles(customFonts) {
  let styleEl = document.getElementById(CUSTOM_FONT_STYLE_ID);
  if (!styleEl) {
    styleEl = document.createElement("style");
    styleEl.id = CUSTOM_FONT_STYLE_ID;
    document.head.appendChild(styleEl);
  }
  styleEl.textContent = "";
  injectedFonts.clear();
  for (const f of customFonts || []) {
    injectCustomFont(f.name, f.path);
  }
}

// ========== 应用设置 ==========
async function applySettings(s) {
  if (!s) return;
  settings = s;
  iconStyle = s.icon_style || "mask";

  const root = document.documentElement;
  root.style.setProperty("--icon-size", `${s.icon_size}px`);
  root.style.setProperty("--name-size", `${s.font_size}px`);

  applyTheme(s.theme || "system");
  rebuildCustomFontStyles(s.custom_fonts);

  let fontCss;
  if (s.font_family && s.font_family.startsWith("custom:")) {
    const name = s.font_family.slice(7);
    const info = (s.custom_fonts || []).find((f) => f.name === name);
    fontCss = info
      ? `"${name}", "Segoe UI", system-ui, sans-serif`
      : FONT_MAP.segoe;
  } else {
    fontCss = FONT_MAP[s.font_family] || FONT_MAP.segoe;
  }
  root.style.setProperty("--font-family", fontCss);
  document.body.style.fontFamily = fontCss;

  updateBgAlpha();
}

// ========== 初始化 ==========
async function init() {
  applyTheme("system");
  await loadSystemInfo();

  titleEl.textContent = IS_LAUNCHER ? "软件面板" : "文件面板";
  document.querySelector(".panel").classList.add(
    IS_LAUNCHER ? "edge-left" : "edge-right"
  );

  if (IS_LAUNCHER) {
    btnFolder.classList.add("hidden");
    btnFile.classList.add("hidden");
  } else {
    btnFolder.classList.remove("hidden");
    btnFile.classList.remove("hidden");
    scopeBtn.classList.add("hidden");
    searchWrap.classList.add("hidden");
  }

  try {
    const s = await invoke("get_settings");
    await applySettings(s);
  } catch (e) {
    console.warn("get_settings failed:", e);
  }

  applyView();
  applyScope();
  loadData();
  initDrag();
}

// ========== 设置按钮 ==========
settingsBtn.addEventListener("click", () => {
  invoke("show_settings").catch((e) => console.error("show_settings failed:", e));
});

// ========== 视图切换 ==========
function applyView() {
  listEl.classList.toggle("list", viewMode === "list");
  if (viewMode === "grid") {
    viewBtn.innerHTML = ICON_LIST;
    viewBtn.title = "切换为列表视图";
  } else {
    viewBtn.innerHTML = ICON_GRID;
    viewBtn.title = "切换为网格视图";
  }
}

viewBtn.addEventListener("click", () => {
  viewMode = viewMode === "grid" ? "list" : "grid";
  localStorage.setItem(`${LS_KEY}.view`, viewMode);
  applyView();
});

// ========== 范围切换 ==========
function applyScope() {
  if (!IS_LAUNCHER) return;
  if (showAll) {
    scopeBtn.innerHTML = ICON_SHOW_ALL;
    scopeBtn.classList.add("active");
    scopeBtn.title = "当前：显示全部应用（点击仅显示桌面）";
  } else {
    scopeBtn.innerHTML = ICON_DESKTOP_ONLY;
    scopeBtn.classList.remove("active");
    scopeBtn.title = "当前：仅显示桌面快捷方式（点击显示全部）";
  }
}

scopeBtn.addEventListener("click", async () => {
  showAll = !showAll;
  localStorage.setItem(`${LS_KEY}.showAll`, showAll ? "1" : "0");
  applyScope();
  await loadData();
});

// ============================================================================
//  全量应用
// ============================================================================
async function ensureAllItems() {
  if (allItemsCache) return allItemsCache;
  if (allItemsPromise) return allItemsPromise;

  allItemsPromise = (async () => {
    try {
      const [desk, apps, uwp] = await Promise.all([
        invoke("scan_desktop"),
        invoke("scan_start"),
        invoke("scan_uwp"),
      ]);
      const seen = new Set();
      const merged = [];
      for (const it of [...desk, ...apps, ...uwp]) {
        const k = it.name.toLowerCase();
        if (seen.has(k)) continue;
        seen.add(k);
        merged.push(it);
      }
      allItemsCache = merged;
      return merged;
    } catch (e) {
      console.warn("ensureAllItems failed:", e);
      return items;
    } finally {
      allItemsPromise = null;
    }
  })();

  return allItemsPromise;
}

// ============================================================================
//  搜索框
// ============================================================================
function updateSearchClear() {
  if (searchInput.value.length > 0) {
    searchWrap.classList.add("has-value");
  } else {
    searchWrap.classList.remove("has-value");
  }
}

searchInput.addEventListener("input", () => {
  currentQuery = searchInput.value;
  updateSearchClear();
  render(currentQuery);
});

searchInput.addEventListener("keydown", (e) => {
  e.stopPropagation();
  if (e.key === "Escape") {
    e.preventDefault();
    if (searchInput.value) {
      searchInput.value = "";
      currentQuery = "";
      updateSearchClear();
      render("");
    } else {
      searchInput.blur();
    }
  } else if (e.key === "Enter") {
    e.preventDefault();
    const cells = listEl.querySelectorAll(".cell");
    if (cells.length) {
      const cell = cells[selected] || cells[0];
      if (cell) cell.dispatchEvent(new Event("dblclick"));
    }
  } else if (
    e.key === "ArrowDown" || e.key === "ArrowUp" ||
    e.key === "ArrowLeft" || e.key === "ArrowRight"
  ) {
    e.preventDefault();
    handleArrowKey(e.key);
  }
});

searchClear.addEventListener("click", () => {
  searchInput.value = "";
  currentQuery = "";
  updateSearchClear();
  searchInput.focus();
  render("");
});

// ========== 拼音 ==========
const initialsCache = new Map();
let pinyinFn = null;

async function loadPinyin() {
  try {
    const mod = await import("pinyin-pro");
    pinyinFn = mod.pinyin;
  } catch {
    pinyinFn = null;
  }
}

function initials(name) {
  if (!pinyinFn) return "";
  if (initialsCache.has(name)) return initialsCache.get(name);
  let s = "";
  try {
    s = pinyinFn(name, { pattern: "first", toneType: "none", type: "array" })
      .join("")
      .toLowerCase();
  } catch { s = ""; }
  initialsCache.set(name, s);
  return s;
}

function matchName(name, q) {
  if (!q) return true;
  const n = name.toLowerCase();
  return n.includes(q) || initials(name).includes(q);
}

// ========== 字母图标 ==========
function letterIcon(name) {
  let h = 0;
  for (let i = 0; i < name.length; i++) {
    h = (h * 31 + name.charCodeAt(i)) >>> 0;
  }
  h = h % 360;
  const h2 = (h + 50) % 360;
  const c1 = `hsl(${h}, 55%, 45%)`;
  const c2 = `hsl(${h2}, 50%, 35%)`;
  const el = document.createElement("div");
  el.className = "letter";
  el.style.background = `linear-gradient(135deg, ${c1} 0%, ${c2} 100%)`;
  el.textContent = (name[0] || "?").toUpperCase();
  return el;
}

// ========== 图标缓存 ==========
const iconCache = new Map();

async function loadIcon(item) {
  if (iconStyle === "redraw") return null;
  if (iconCache.has(item.path)) return iconCache.get(item.path);
  try {
    const url = await invoke("icon", { path: item.path });
    iconCache.set(item.path, url);
    return url;
  } catch (e) {
    console.warn("icon failed:", item.path, e);
    iconCache.set(item.path, null);
    return null;
  }
}

function makeIconImg(url) {
  const img = document.createElement("img");
  img.src = url;
  img.alt = "";
  img.draggable = false;
  img.className = iconStyle === "real" ? "icon-real" : "icon-mask";
  return img;
}

// ========== 删除到回收站 ==========
async function deleteToTrash(item) {
  const ok = await showConfirm(
    "移动到回收站",
    `确定要把「${item.name}」移动到回收站吗？`,
    "删除"
  );
  if (!ok) return;
  try {
    await invoke("delete_to_trash", { path: item.path });
    iconCache.delete(item.path);
    await loadData();
  } catch (e) {
    console.error("delete_to_trash failed:", e);
    await showConfirm("删除失败", String(e), "知道了");
  }
}

// ========== 重命名 ==========
async function renameItem(item) {
  const newName = await showInput("重命名", item.name);
  if (!newName || newName === item.name) return;
  try {
    await invoke("rename_item", { path: item.path, new_name: newName });
    iconCache.delete(item.path);
    await loadData();
  } catch (e) {
    console.error("rename failed:", e);
    await showConfirm("重命名失败", String(e) || "未知错误", "知道了");
  }
}

function canRename(item) {
  if (!item || !item.path) return false;
  if (item.path.startsWith("shell:")) return false;
  if (IS_LAUNCHER) {
    return item.kind === "shortcut" || item.kind === "exe";
  }
  return true;
}

// ============================================================================
//  拖拽移动
// ============================================================================
const DRAG_THRESHOLD = 5;
let drag = null;
let justDragged = false;

function initDrag() {
  document.addEventListener("mousedown", onDragMouseDown);
  document.addEventListener("mousemove", onDragMouseMove);
  document.addEventListener("mouseup", onDragMouseUp);
  document.addEventListener(
    "click",
    (e) => {
      if (justDragged) {
        justDragged = false;
        e.stopPropagation();
        e.preventDefault();
      }
    },
    true
  );
}

function onDragMouseDown(e) {
  if (IS_LAUNCHER) return;
  if (e.button !== 0) return;
  if (
    confirmOverlay.classList.contains("show") ||
    inputOverlay.classList.contains("show")
  ) return;
  if (e.target.closest("button")) return;

  const cell = e.target.closest(".cell");
  if (!cell) return;

  const idx = Number(cell.dataset.index);
  const item = currentVisible[idx];
  if (!item) return;

  drag = {
    item,
    sourceCell: cell,
    startX: e.clientX,
    startY: e.clientY,
    activated: false,
    ghost: null,
    hover: null,
  };
}

function onDragMouseMove(e) {
  if (!drag) return;
  if (!drag.activated) {
    const dx = e.clientX - drag.startX;
    const dy = e.clientY - drag.startY;
    if (Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    drag.activated = true;
    drag.sourceCell.classList.add("dragging");
    const ghost = document.createElement("div");
    ghost.className = "drag-ghost";
    ghost.textContent = drag.item.name;
    document.body.appendChild(ghost);
    drag.ghost = ghost;
  }
  drag.ghost.style.left = (e.clientX + 14) + "px";
  drag.ghost.style.top = (e.clientY + 14) + "px";

  const el = document.elementFromPoint(e.clientX, e.clientY);
  const hoverCell = el?.closest?.(".cell");

  let target = null;
  if (hoverCell) {
    const idx = Number(hoverCell.dataset.index);
    const item = currentVisible[idx];
    if (item && item.is_dir && item.path !== drag.item.path) {
      target = { cell: hoverCell, item };
    }
  }
  if (drag.hover?.cell !== target?.cell) {
    if (drag.hover) drag.hover.cell.classList.remove("drop-target");
    if (target) target.cell.classList.add("drop-target");
    drag.hover = target;
  }
  e.preventDefault();
}

async function onDragMouseUp(e) {
  if (!drag) return;
  const d = drag;
  drag = null;
  if (!d.activated) return;
  justDragged = true;
  d.sourceCell.classList.remove("dragging");
  if (d.ghost) d.ghost.remove();
  if (d.hover) d.hover.cell.classList.remove("drop-target");
  if (!d.hover) return;

  const dstDir = d.hover.item.path;
  const srcPath = d.item.path;
  try {
    await invoke("move_item", { src: srcPath, dst_dir: dstDir });
    iconCache.delete(srcPath);
    await loadData();
  } catch (err) {
    console.error("move failed:", err);
    await showConfirm("移动失败", String(err) || "未知错误", "知道了");
  }
}

// ========== 渲染 ==========
async function render(query = "") {
  const seq = ++renderSeq;
  const q = query.trim().toLowerCase();

  let source = items;
  let loadingHint = null;

  if (q && IS_LAUNCHER) {
    if (!allItemsCache) {
      loadingHint = document.createElement("div");
      loadingHint.className = "empty";
      loadingHint.textContent = "搜索中…";
      listEl.innerHTML = "";
      listEl.appendChild(loadingHint);
    }
    source = await ensureAllItems();
    if (seq !== renderSeq) return;
  }

  const visible = source.filter((it) => matchName(it.name, q));
  currentVisible = visible;
  listEl.innerHTML = "";

  if (!visible.length) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = source.length ? "无匹配结果" : "暂无内容";
    listEl.appendChild(empty);
    return;
  }

  const frag = document.createDocumentFragment();
  const pending = [];

  for (let i = 0; i < visible.length; i++) {
    const it = visible[i];
    const cell = document.createElement("div");
    cell.className = "cell";
    cell.dataset.index = String(i);
    cell.title = it.name;
    cell.appendChild(letterIcon(it.name));

    const nm = document.createElement("span");
    nm.className = "name";
    nm.textContent = it.name;
    cell.appendChild(nm);

    cell.addEventListener("click", () => selectIndex(i));
    cell.addEventListener("dblclick", () => activate(it));

    if (canRename(it)) {
      const renameBtn = document.createElement("button");
      renameBtn.className = "rename-btn";
      renameBtn.title = "重命名";
      renameBtn.setAttribute("aria-label", "重命名");
      renameBtn.innerHTML = ICON_RENAME;
      renameBtn.addEventListener("click", (e) => {
        e.stopPropagation(); e.preventDefault(); renameItem(it);
      });
      renameBtn.addEventListener("dblclick", (e) => {
        e.stopPropagation(); e.preventDefault();
      });
      cell.appendChild(renameBtn);
    }

    const delBtn = document.createElement("button");
    delBtn.className = "delete-btn";
    delBtn.title = "移动到回收站";
    delBtn.setAttribute("aria-label", "移动到回收站");
    delBtn.innerHTML = ICON_DELETE;
    delBtn.addEventListener("click", (e) => {
      e.stopPropagation(); e.preventDefault(); deleteToTrash(it);
    });
    delBtn.addEventListener("dblclick", (e) => {
      e.stopPropagation(); e.preventDefault();
    });
    cell.appendChild(delBtn);

    frag.appendChild(cell);
    if (iconStyle !== "redraw") pending.push({ cell, item: it });
  }

  if (seq !== renderSeq) return;

  listEl.appendChild(frag);
  selectIndex(0);

  (async () => {
    for (const { cell, item } of pending) {
      if (seq !== renderSeq) return;
      const url = await loadIcon(item);
      if (!url) continue;
      const img = makeIconImg(url);
      const first = cell.firstChild;
      if (first && (first.classList?.contains("letter") || first.tagName === "IMG")) {
        cell.replaceChild(img, first);
      }
    }
  })();
}

function selectIndex(i) {
  const cells = [...listEl.querySelectorAll(".cell")];
  if (!cells.length) return;
  i = Math.max(0, Math.min(cells.length - 1, i));
  cells.forEach((c) => c.classList.remove("selected"));
  cells[i].classList.add("selected");
  cells[i].scrollIntoView({ block: "nearest" });
  selected = i;
}

async function activate(item) {
  try { await invoke("open", { path: item.path }); }
  catch (e) { console.error("open failed:", e); }
  await invoke("hide_panel_cmd", { label: PANEL }).catch(() => {});
}

async function loadData() {
  allItemsCache = null;
  try {
    if (IS_LAUNCHER) {
      const desk = await invoke("scan_desktop");
      if (showAll) {
        const [apps, uwp] = await Promise.all([
          invoke("scan_start"),
          invoke("scan_uwp"),
        ]);
        const seen = new Set();
        items = [];
        for (const it of [...desk, ...apps, ...uwp]) {
          const k = it.name.toLowerCase();
          if (seen.has(k)) continue;
          seen.add(k);
          items.push(it);
        }
      } else {
        items = desk;
      }
    } else {
      const [folders, files] = await invoke("scan_files");
      items = [...folders, ...files];
    }
  } catch (e) {
    console.error("scan failed:", e);
    items = [];
  }
  await render(currentQuery);
}

function columnCount() {
  if (viewMode === "list") return 1;
  const w = listEl.clientWidth;
  const cw = 88;
  return Math.max(1, Math.floor(w / cw));
}

function handleArrowKey(key) {
  switch (key) {
    case "ArrowRight": selectIndex(selected + 1); break;
    case "ArrowLeft":  selectIndex(selected - 1); break;
    case "ArrowDown":  selectIndex(selected + columnCount()); break;
    case "ArrowUp":    selectIndex(selected - columnCount()); break;
  }
}

document.addEventListener("keydown", async (e) => {
  if (confirmOverlay.classList.contains("show")) {
    if (e.key === "Escape") closeConfirm(false);
    else if (e.key === "Enter") closeConfirm(true);
    e.preventDefault(); e.stopPropagation();
    return;
  }
  if (inputOverlay.classList.contains("show")) {
    if (e.key === "Enter") {
      e.preventDefault();
      const v = inputField.value.trim();
      closeInput(v ? v : null);
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeInput(null);
    }
    e.stopPropagation();
    return;
  }
  if (document.activeElement === searchInput) return;

  const cells = listEl.querySelectorAll(".cell");
  if (!cells.length) return;

  switch (e.key) {
    case "ArrowRight":
    case "ArrowLeft":
    case "ArrowDown":
    case "ArrowUp":
      handleArrowKey(e.key);
      e.preventDefault();
      break;
    case "Enter": {
      const cell = cells[selected];
      if (cell) cell.dispatchEvent(new Event("dblclick"));
      e.preventDefault();
      break;
    }
    case "Escape":
      invoke("hide_panel_cmd", { label: PANEL }).catch(() => {});
      e.preventDefault();
      break;
    case "F2": {
      const cell = cells[selected];
      if (cell) {
        const matched = items.find((x) => x.name === cell.title);
        if (matched && canRename(matched)) renameItem(matched);
      }
      e.preventDefault();
      break;
    }
  }
});

btnFolder?.addEventListener("click", async () => {
  const name = await showInput("新建文件夹", "新建文件夹");
  if (!name) return;
  try {
    await invoke("mk_folder", { name, parent: null });
    iconCache.clear();
    await loadData();
  } catch (e) {
    console.error("mk_folder failed:", e);
    await showConfirm("新建文件夹失败", String(e), "知道了");
  }
});

btnFile?.addEventListener("click", async () => {
  const name = await showInput("新建文件", "新建文本文档.txt");
  if (!name) return;
  try {
    await invoke("mk_file", { name, parent: null });
    iconCache.clear();
    await loadData();
  } catch (e) {
    console.error("mk_file failed:", e);
    await showConfirm("新建文件失败", String(e), "知道了");
  }
});

// ========== 监听设置变化 ==========
listen("settings-changed", async (e) => {
  const oldStyle = iconStyle;
  await applySettings(e.payload);
  if (oldStyle !== iconStyle) {
    iconCache.clear();
  }
  render(currentQuery);
});

// ========== 监听托盘「刷新」 ==========
listen("refresh-requested", async () => {
  console.log("[refresh] 收到刷新请求");
  iconCache.clear();
  allItemsCache = null;
  allItemsPromise = null;
  await loadData();
});

// ========== 启动 ==========
init();
loadPinyin();