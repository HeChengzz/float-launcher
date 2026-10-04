import "./settings.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const closeBtn = document.getElementById("btn-close");
const versionEl = document.getElementById("version");

const rngIconSize = document.getElementById("rng-icon-size");
const valIconSize = document.getElementById("val-icon-size");
const rngFontSize = document.getElementById("rng-font-size");
const valFontSize = document.getElementById("val-font-size");
const rngMicaStrength = document.getElementById("rng-mica-strength");
const valMicaStrength = document.getElementById("val-mica-strength");
const rngOpacity = document.getElementById("rng-opacity");
const valOpacity = document.getElementById("val-opacity");
const selFontFamily = document.getElementById("sel-font-family");
const optCustom = document.getElementById("opt-custom");
const chkAutostart = document.getElementById("chk-autostart");
const chkPauseFullscreen = document.getElementById("chk-pause-fullscreen");
const btnUploadFont = document.getElementById("btn-upload-font");
const fontHint = document.getElementById("font-hint");

let settings = null;

// ============================================================================
//  主题
// ============================================================================
const systemLightMQ = window.matchMedia("(prefers-color-scheme: light)");

function resolveTheme(theme) {
  if (theme === "light" || theme === "dark") return theme;
  return systemLightMQ.matches ? "light" : "dark";
}

function applyTheme(theme) {
  document.documentElement.setAttribute("data-theme", resolveTheme(theme));
  updateBgAlpha();
}

let currentTheme = "system";
systemLightMQ.addEventListener("change", () => {
  if (currentTheme === "system") applyTheme("system");
});

// ============================================================================
//  ★ 计算背景 + 透明度
// ============================================================================
function updateBgAlpha() {
  const theme = document.documentElement.getAttribute("data-theme") || "dark";
  const effect = document.documentElement.getAttribute("data-effect") || "none";
  const ms = (settings?.mica_strength ?? 30) / 100;
  const opacity = (settings?.opacity ?? 100) / 100;

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

  // ★ 把透明度乘进背景 alpha
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
}
// ============================================================================
//  系统材质
// ============================================================================
async function loadSystemInfo() {
  try {
    const info = await invoke("get_system_info");
    document.documentElement.setAttribute("data-effect", info.effect || "none");
  } catch (e) {
    document.documentElement.setAttribute("data-effect", "none");
  }
  updateBgAlpha();
}

listen("system-effect-changed", (e) => {
  document.documentElement.setAttribute("data-effect", e.payload || "none");
  updateBgAlpha();
});

// ========== 关闭 ==========
closeBtn.addEventListener("click", () => {
  invoke("hide_settings").catch(() => {});
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    invoke("hide_settings").catch(() => {});
    e.preventDefault();
  }
});

// ========== 设置写入 ==========
async function setSetting(key, value) {
  try { await invoke("set_setting", { key, value }); }
  catch (e) { console.error("set_setting failed:", key, e); }
}

// ========== 自定义字体下拉 ==========
function renderCustomFonts(fonts) {
  optCustom.innerHTML = "";
  if (!fonts || !fonts.length) { optCustom.hidden = true; return; }
  optCustom.hidden = false;
  for (const f of fonts) {
    const opt = document.createElement("option");
    opt.value = "custom:" + f.name;
    opt.textContent = f.name;
    optCustom.appendChild(opt);
  }
}

// ========== 分段 ==========
document.querySelectorAll(".segmented").forEach((seg) => {
  const key = seg.dataset.key;
  seg.addEventListener("click", (e) => {
    const btn = e.target.closest("button");
    if (!btn) return;
    [...seg.querySelectorAll("button")].forEach((b) => {
      b.classList.toggle("active", b === btn);
    });
    const value = btn.dataset.value;
    if (key === "theme") {
      currentTheme = value;
      applyTheme(value);
    }
    setSetting(key, value);
  });
});

// ========== 图标大小 ==========
rngIconSize.addEventListener("input", () => {
  valIconSize.textContent = rngIconSize.value;
});
rngIconSize.addEventListener("change", () => {
  setSetting("icon_size", Number(rngIconSize.value));
});

// ========== 字体大小 ==========
rngFontSize.addEventListener("input", () => {
  valFontSize.textContent = rngFontSize.value;
});
rngFontSize.addEventListener("change", () => {
  setSetting("font_size", Number(rngFontSize.value));
});

// ========== Mica 强度 ==========
rngMicaStrength.addEventListener("input", () => {
  valMicaStrength.textContent = rngMicaStrength.value + "%";
  if (settings) settings.mica_strength = Number(rngMicaStrength.value);
  updateBgAlpha();
});
rngMicaStrength.addEventListener("change", () => {
  setSetting("mica_strength", Number(rngMicaStrength.value));
});

// ★ 面板透明度
rngOpacity.addEventListener("input", () => {
  valOpacity.textContent = rngOpacity.value + "%";
  if (settings) settings.opacity = Number(rngOpacity.value);
  updateBgAlpha();
});
rngOpacity.addEventListener("change", () => {
  setSetting("opacity", Number(rngOpacity.value));
});

// ========== 字体样式 ==========
selFontFamily.addEventListener("change", () => {
  setSetting("font_family", selFontFamily.value);
});

// ========== 字体上传 ==========
btnUploadFont.addEventListener("click", async () => {
  btnUploadFont.disabled = true;
  fontHint.textContent = "选择中…";
  try {
    const result = await invoke("upload_font");
    if (!result) fontHint.textContent = "已取消";
    else fontHint.textContent = `已上传：${result.name}`;
    await loadSettings();
  } catch (e) {
    fontHint.textContent = "上传失败：" + e;
  } finally {
    btnUploadFont.disabled = false;
    setTimeout(() => {
      fontHint.textContent = "支持 .ttf / .otf / .woff / .woff2";
    }, 3000);
  }
});

// ========== 游戏全屏时暂停 ==========
chkPauseFullscreen.addEventListener("change", async () => {
  const value = chkPauseFullscreen.checked;
  chkPauseFullscreen.disabled = true;
  try { await invoke("set_setting", { key: "pause_on_fullscreen", value }); }
  catch (e) { chkPauseFullscreen.checked = !value; }
  finally { chkPauseFullscreen.disabled = false; }
});

// ========== 开机自启 ==========
chkAutostart.addEventListener("change", async () => {
  const value = chkAutostart.checked;
  chkAutostart.disabled = true;
  try { await invoke("set_autostart", { value }); }
  catch (e) { chkAutostart.checked = !value; }
  finally { chkAutostart.disabled = false; }
});

// ========== 加载初始值 ==========
async function loadSettings() {
  try {
    const s = await invoke("get_settings");
    settings = s;

    currentTheme = s.theme || "system";
    applyTheme(currentTheme);
    const themeSeg = document.querySelector('.segmented[data-key="theme"]');
    if (themeSeg) {
      [...themeSeg.querySelectorAll("button")].forEach((b) => {
        b.classList.toggle("active", b.dataset.value === currentTheme);
      });
    }
    const styleSeg = document.querySelector('.segmented[data-key="icon_style"]');
    if (styleSeg) {
      [...styleSeg.querySelectorAll("button")].forEach((b) => {
        b.classList.toggle("active", b.dataset.value === s.icon_style);
      });
    }

    rngIconSize.value = s.icon_size;
    valIconSize.textContent = s.icon_size;
    rngFontSize.value = s.font_size;
    valFontSize.textContent = s.font_size;

    const ms = s.mica_strength ?? 30;
    rngMicaStrength.value = ms;
    valMicaStrength.textContent = ms + "%";

    const op = s.opacity ?? 100;
    rngOpacity.value = op;
    valOpacity.textContent = op + "%";

    renderCustomFonts(s.custom_fonts);
    selFontFamily.value = s.font_family;

    chkPauseFullscreen.checked = !!s.pause_on_fullscreen;
    chkAutostart.checked = !!s.autostart;
    versionEl.textContent = "v" + (s.version || "0.1.0");

    updateBgAlpha();
  } catch (e) {
    console.error("get_settings failed:", e);
  }
}

// ========== 启动 ==========
(async () => {
  applyTheme("system");
  await loadSystemInfo();
  await loadSettings();
})();