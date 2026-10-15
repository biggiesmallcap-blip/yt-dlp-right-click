import {
  DEFAULT_SETTINGS,
  HOST_NAME,
  hostSettings,
  normalizeSettings,
  timeAgo
} from "./shared.js";

const $ = (id) => document.getElementById(id);
const form = $("settings-form");
const saveState = $("save-state");

const TEXT_FIELDS = ["ytDlpPath", "ffmpegPath", "downloadDir", "cookiesFile", "jsRuntimePath"];
const SELECT_FIELDS = ["cookieMode", "cookieBrowser", "updateChannel", "jsRuntime"];
const CHECKBOX_FIELDS = ["embedAudioMetadata", "notifyOnFinish", "autoUpdateYtDlp", "checkExtensionUpdates"];

let saveTimer = 0;

init().catch((error) => showSaveState(error.message || String(error), false));

async function init() {
  $("version").textContent = `v${chrome.runtime.getManifest().version}`;
  $("extensionId").value = chrome.runtime.id;
  $("installCommand").value = `.\\scripts\\install-native-host.ps1 -ExtensionId "${chrome.runtime.id}"`;

  const stored = await chrome.storage.local.get({
    settings: DEFAULT_SETTINGS,
    lastYtDlpUpdateResult: null,
    extensionUpdate: null
  });
  fillForm(normalizeSettings(stored.settings));
  renderYtDlpUpdateState(stored.lastYtDlpUpdateResult);
  renderExtensionUpdateState(stored.extensionUpdate);
  refreshHostState(false);
}

// Autosave ------------------------------------------------------------------

form.addEventListener("submit", (event) => event.preventDefault());
form.addEventListener("change", () => scheduleSave(0));
form.addEventListener("input", (event) => {
  if (event.target.matches("input[type='text']")) scheduleSave(600);
});

function scheduleSave(delay) {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(saveSettings, delay);
}

async function saveSettings() {
  clearTimeout(saveTimer);
  const { settings: current } = await chrome.storage.local.get({ settings: DEFAULT_SETTINGS });
  const settings = normalizeSettings({ ...current, ...readForm() });
  await chrome.storage.local.set({ settings });
  showSaveState("Saved", true);
  return settings;
}

function fillForm(settings) {
  for (const key of TEXT_FIELDS) $(key).value = settings[key];
  for (const key of SELECT_FIELDS) $(key).value = settings[key];
  for (const key of CHECKBOX_FIELDS) $(key).checked = Boolean(settings[key]);
  $("displayModeAdvanced").checked = settings.displayMode === "advanced";
}

function readForm() {
  const values = {};
  for (const key of TEXT_FIELDS) values[key] = $(key).value.trim();
  for (const key of SELECT_FIELDS) values[key] = $(key).value;
  for (const key of CHECKBOX_FIELDS) values[key] = $(key).checked;
  values.displayMode = $("displayModeAdvanced").checked ? "advanced" : "simple";
  return values;
}

let saveStateTimer = 0;
function showSaveState(text, ok) {
  saveState.textContent = text;
  saveState.classList.toggle("bad", !ok);
  clearTimeout(saveStateTimer);
  if (ok) saveStateTimer = setTimeout(() => { saveState.textContent = ""; }, 1800);
}

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== "local") return;
  if (changes.lastYtDlpUpdateResult) renderYtDlpUpdateState(changes.lastYtDlpUpdateResult.newValue);
  if (changes.extensionUpdate) renderExtensionUpdateState(changes.extensionUpdate.newValue);
  // The popup can change cookie mode and the logs toggle while this page is
  // open. Mirror those so the next autosave does not write stale values back,
  // but never touch the field being edited.
  if (changes.settings?.newValue) {
    const next = normalizeSettings(changes.settings.newValue);
    for (const key of SELECT_FIELDS) {
      if (document.activeElement !== $(key)) $(key).value = next[key];
    }
    for (const key of CHECKBOX_FIELDS) {
      if (document.activeElement !== $(key)) $(key).checked = Boolean(next[key]);
    }
    $("displayModeAdvanced").checked = next.displayMode === "advanced";
  }
});

// Check setup -------------------------------------------------------------

$("check").addEventListener("click", async () => {
  const button = $("check");
  const box = $("check-result");
  button.disabled = true;
  renderResult(box, "", [paragraph("Checking. This runs yt-dlp and ffmpeg once...")]);
  try {
    const settings = await saveSettings();
    const response = await sendNativeMessage({ type: "validate", settings: hostSettings(settings) });
    const details = definitionList([
      ["Native host", response?.hostVersion ? `v${response.hostVersion}` : "v1.0.0 (outdated)"],
      ["yt-dlp", response?.ytDlpVersion || "not checked"],
      ["ffmpeg", response?.ffmpegVersion || "not checked"]
    ]);
    const nodes = [details];
    if (response?.errors?.length) nodes.unshift(list(response.errors));
    if (response?.warnings?.length) nodes.push(list(response.warnings));
    if (!response?.hostVersion) {
      nodes.push(paragraph("This native host predates version checks. Rebuild it with cargo build --release, or install the latest native host release."));
    }
    const tone = !response?.ok ? "bad" : response.warnings?.length || !response.hostVersion ? "warn" : "good";
    if (response?.ok && !response.warnings?.length) nodes.unshift(paragraph("Everything works."));
    renderResult(box, tone, nodes);
  } catch (error) {
    renderResult(box, "bad", [paragraph(error.message || String(error))]);
  } finally {
    button.disabled = false;
    refreshHostState(true);
  }
});

// Updates -----------------------------------------------------------------

$("update-ytdlp").addEventListener("click", async () => {
  const button = $("update-ytdlp");
  const box = $("update-result");
  button.disabled = true;
  renderResult(box, "", [paragraph("Updating yt-dlp...")]);
  try {
    await saveSettings();
    const response = await chrome.runtime.sendMessage({ type: "updateYtDlp" });
    const nodes = [paragraph(response?.ok ? "yt-dlp is up to date." : `Update failed (exit ${response?.exitCode ?? "unknown"}).`)];
    if (response?.output) {
      const pre = document.createElement("pre");
      pre.textContent = response.output.trim();
      nodes.push(pre);
    }
    renderResult(box, response?.ok ? "good" : "bad", nodes);
  } catch (error) {
    renderResult(box, "bad", [paragraph(error.message || String(error))]);
  } finally {
    button.disabled = false;
  }
});

function renderYtDlpUpdateState(result) {
  $("ytdlp-update-state").textContent = result?.at
    ? `Last ${result.source === "manual" ? "manual" : "automatic"} update ${timeAgo(result.at)}: ${result.ok ? "succeeded" : "failed"}.`
    : "No update has run yet.";
}

$("check-extension-update").addEventListener("click", async () => {
  const button = $("check-extension-update");
  button.disabled = true;
  $("extension-update-state").textContent = "Checking GitHub...";
  try {
    const response = await chrome.runtime.sendMessage({ type: "checkExtensionUpdate" });
    if (response?.ok === false) throw new Error(response.message);
    renderExtensionUpdateState(response);
  } catch (error) {
    $("extension-update-state").textContent = `Could not check: ${error.message || error}`;
  } finally {
    button.disabled = false;
  }
});

function renderExtensionUpdateState(state) {
  const box = $("extension-update-state");
  box.textContent = "";
  if (!state?.checkedAt) {
    box.textContent = "Not checked yet.";
    return;
  }
  if (state.available) {
    const link = document.createElement("a");
    link.href = state.url;
    link.target = "_blank";
    link.rel = "noreferrer";
    link.textContent = `Version ${state.latestVersion} is available`;
    box.append(link, ` (you have ${state.currentVersion}).`);
  } else {
    box.textContent = `You have the latest version. Checked ${timeAgo(new Date(state.checkedAt).toISOString())}.`;
  }
}

// Native host -------------------------------------------------------------

async function refreshHostState(force) {
  const box = $("host-state");
  try {
    const info = await chrome.runtime.sendMessage({ type: "getHostInfo", force });
    if (!info?.ok) {
      renderResult(box, "bad", [
        paragraph(info?.message || "The native host did not respond."),
        paragraph("Copy the install command below and run it from the project folder, then reload this page.")
      ]);
    } else if (info.outdated) {
      renderResult(box, "warn", [
        paragraph(`Installed: v${info.hostVersion}. This extension is v${info.extensionVersion}, so some presets will fail until the host is updated.`),
        paragraph("From source: cd native-host, then cargo build --release. From a release: run the newest install-native-host.cmd.")
      ]);
    } else {
      renderResult(box, "good", [paragraph(`Installed and responding (v${info.hostVersion}).`)]);
    }
  } catch (error) {
    renderResult(box, "bad", [paragraph(error.message || String(error))]);
  }
}

document.addEventListener("click", async (event) => {
  const id = event.target.closest("button[data-copy]")?.dataset.copy;
  if (!id) return;
  try {
    await navigator.clipboard.writeText($(id).value);
    showSaveState("Copied", true);
  } catch (error) {
    showSaveState(`Copy failed: ${error.message || error}`, false);
  }
});

// Helpers -------------------------------------------------------------------

function sendNativeMessage(message) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendNativeMessage(HOST_NAME, message, (response) => {
      const error = chrome.runtime.lastError;
      if (error) {
        reject(new Error(`Could not reach the native host: ${error.message}`));
        return;
      }
      resolve(response);
    });
  });
}

function renderResult(box, tone, nodes) {
  box.hidden = false;
  box.className = `result ${tone}`.trim();
  box.replaceChildren(...nodes);
}

function paragraph(text) {
  const p = document.createElement("p");
  p.style.margin = "0";
  p.textContent = text;
  return p;
}

function list(items) {
  const ul = document.createElement("ul");
  for (const item of items) {
    const li = document.createElement("li");
    li.textContent = item;
    ul.append(li);
  }
  return ul;
}

function definitionList(pairs) {
  const dl = document.createElement("dl");
  for (const [term, value] of pairs) {
    const dt = document.createElement("dt");
    dt.textContent = term;
    const dd = document.createElement("dd");
    dd.textContent = value;
    dl.append(dt, dd);
  }
  return dl;
}
