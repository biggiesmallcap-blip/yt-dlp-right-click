const jobsContainer = document.getElementById("jobs");
const template = document.getElementById("job-template");
const clearButton = document.getElementById("clear");
const settingsButton = document.getElementById("settings");
const cookiesButton = document.getElementById("cookies");
const displayModeInput = document.getElementById("display-mode");
const displayModeWrap = document.getElementById("display-mode-wrap");
const settingsStatus = document.getElementById("settings-status");
const manualPresetInput = document.getElementById("manual-preset");
const manualDownloadButton = document.getElementById("manual-download");

let currentSettings = {};
let currentJobs = [];
let currentPresets = [];

document.addEventListener("DOMContentLoaded", renderFromStorage);
clearButton.addEventListener("click", clearCompleted);
settingsButton.addEventListener("click", openSettings);
cookiesButton.addEventListener("click", toggleCookies);
displayModeInput.addEventListener("change", toggleDisplayMode);
jobsContainer.addEventListener("click", handleJobAction);
manualDownloadButton.addEventListener("click", downloadCurrentSite);
manualPresetInput.addEventListener("change", rememberSelectedPreset);

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName === "local" && changes.jobs) {
    currentJobs = changes.jobs.newValue || [];
    renderJobs(currentJobs);
  }
  if (areaName === "local" && changes.settings) {
    currentSettings = changes.settings.newValue || {};
    applySavedManualPreset();
    renderSettingsStatus(currentSettings);
    renderJobs(currentJobs);
  }
});

async function renderFromStorage() {
  const result = await chrome.storage.local.get({ jobs: [], settings: {} });
  currentSettings = result.settings || {};
  currentJobs = result.jobs || [];
  await renderManualPresets();
  renderSettingsStatus(currentSettings);
  renderJobs(currentJobs);
}

async function renderManualPresets() {
  const response = await chrome.runtime.sendMessage({ type: "getPresets" });
  currentPresets = response?.ok ? response.presets || [] : [];
  manualPresetInput.textContent = "";

  const groups = new Map();
  for (const preset of currentPresets) {
    const group = preset.group || "other";
    if (!groups.has(group)) {
      const optionGroup = document.createElement("optgroup");
      optionGroup.label = groupTitle(group);
      groups.set(group, optionGroup);
      manualPresetInput.append(optionGroup);
    }

    const option = document.createElement("option");
    option.value = preset.id;
    option.textContent = preset.title;
    groups.get(group).append(option);
  }

  const saved = currentSettings.manualPreset || "video_best_mp4";
  applySavedManualPreset();
}

function renderJobs(jobs) {
  jobsContainer.textContent = "";

  if (!jobs.length) {
    const empty = document.createElement("p");
    empty.className = "empty";
    empty.textContent = "No jobs yet.";
    jobsContainer.append(empty);
    return;
  }

  for (const job of jobs) {
    const node = template.content.firstElementChild.cloneNode(true);
    const status = node.querySelector(".status");
    const title = node.querySelector(".title");
    const url = node.querySelector(".url");
    const meta = node.querySelector(".meta");
    const summary = node.querySelector(".summary");
    const actions = node.querySelector(".job-actions");
    const output = node.querySelector(".output");

    status.classList.add(job.status || "unknown");
    title.textContent = `${job.presetTitle || job.preset || "Job"} - ${displayStatus(job)}`;
    node.classList.add(displayMode(currentSettings));

    if (job.url) {
      url.href = job.url;
      url.textContent = job.url;
    } else {
      url.remove();
    }

    meta.textContent = metadataText(job);
    summary.textContent = summaryText(job);
    renderJobActions(actions, job);
    output.textContent = outputText(job);

    jobsContainer.append(node);
  }
}

function renderJobActions(container, job) {
  container.textContent = "";

  if (job.finalPath || job.status === "complete" || job.status === "failed") {
    container.append(createJobButton("Open folder", "open", job.id));
  }

  if (job.status === "failed") {
    container.append(createJobButton("Retry", "retry", job.id));
  }
}

function createJobButton(label, action, jobId) {
  const button = document.createElement("button");
  button.type = "button";
  button.textContent = label;
  button.dataset.action = action;
  button.dataset.jobId = jobId;
  return button;
}

async function handleJobAction(event) {
  const button = event.target.closest("button[data-action][data-job-id]");
  if (!button) {
    return;
  }

  const action = button.dataset.action;
  const jobId = button.dataset.jobId;
  button.disabled = true;

  try {
    const type = action === "retry" ? "retryJob" : "openJobFolder";
    const response = await chrome.runtime.sendMessage({ type, jobId });
    if (!response?.ok) {
      showPopupMessage(response?.message || `${button.textContent} failed.`);
    }
  } catch (error) {
    showPopupMessage(error.message || String(error));
  } finally {
    button.disabled = false;
  }
}

function showPopupMessage(message) {
  settingsStatus.textContent = message;
}

async function downloadCurrentSite() {
  manualDownloadButton.disabled = true;

  try {
    const tab = await currentActiveTab();
    const presetId = manualPresetInput.value || currentPresets[0]?.id || "";
    if (!presetId) {
      showPopupMessage("No download preset is available.");
      return;
    }

    await rememberSelectedPreset();

    const response = await chrome.runtime.sendMessage({
      type: "startDownload",
      presetId,
      url: tab?.url || ""
    });

    if (!response?.ok) {
      showPopupMessage(response?.message || "Could not start download.");
      return;
    }

    showPopupMessage(`Started ${presetTitle(presetId)} for this site.`);
  } catch (error) {
    showPopupMessage(error.message || String(error));
  } finally {
    manualDownloadButton.disabled = false;
  }
}

function currentActiveTab() {
  return new Promise((resolve, reject) => {
    chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
      const error = chrome.runtime.lastError;
      if (error) {
        reject(new Error(error.message));
        return;
      }
      resolve(tabs?.[0] || null);
    });
  });
}

async function rememberSelectedPreset() {
  const presetId = manualPresetInput.value || currentPresets[0]?.id || "";
  if (!presetId) {
    return;
  }

  const result = await chrome.storage.local.get({ settings: {} });
  const settings = result.settings || {};
  await chrome.storage.local.set({
    settings: {
      ...settings,
      manualPreset: presetId
    }
  });
}

function applySavedManualPreset() {
  const saved = currentSettings.manualPreset || "video_best_mp4";
  if (currentPresets.some((preset) => preset.id === saved)) {
    manualPresetInput.value = saved;
  }
}

function presetTitle(presetId) {
  return currentPresets.find((preset) => preset.id === presetId)?.title || "download";
}

function groupTitle(group) {
  if (group === "video") {
    return "Video";
  }
  if (group === "audio") {
    return "Audio";
  }
  if (group === "file") {
    return "File";
  }
  if (group === "playlist") {
    return "Playlist";
  }
  return "Other";
}

function metadataText(job) {
  const parts = [];
  if (job.pid) {
    parts.push(`pid ${job.pid}`);
  }
  if (job.exitCode !== undefined && job.exitCode !== null) {
    parts.push(`exit ${job.exitCode}`);
  }
  if (job.logPath) {
    parts.push(job.logPath);
  }
  if (job.startedAt) {
    parts.push(new Date(job.startedAt).toLocaleString());
  }
  return parts.join(" | ");
}

function outputText(job) {
  const lines = job.output || [];
  if (!lines.length) {
    return "Waiting for output...";
  }

  return lines
    .slice(-60)
    .map((entry) => `[${entry.stream || "out"}] ${entry.line || ""}`)
    .join("\n");
}

function displayStatus(job) {
  if (job.status === "complete" && Number(job.exitCode || 0) !== 0) {
    return "complete with warning";
  }
  return job.status || "unknown";
}

async function clearCompleted() {
  await chrome.runtime.sendMessage({ type: "clearCompletedJobs" });
}

function openSettings() {
  chrome.runtime.openOptionsPage();
}

async function toggleCookies() {
  const result = await chrome.storage.local.get({ settings: {} });
  const settings = result.settings || {};
  const current = normalizedCookieMode(settings);
  const nextMode = current === "auto" ? "never" : current === "never" ? "always" : "auto";
  await chrome.storage.local.set({
    settings: {
      ...settings,
      cookieMode: nextMode,
      cookiesFromBrowser: false
    }
  });
}

async function toggleDisplayMode() {
  const result = await chrome.storage.local.get({ settings: {} });
  const settings = result.settings || {};
  const nextMode = displayModeInput.checked ? "advanced" : "simple";
  await chrome.storage.local.set({
    settings: {
      ...settings,
      displayMode: nextMode
    }
  });
}

function renderSettingsStatus(settings) {
  const mode = normalizedCookieMode(settings);
  const viewMode = displayMode(settings);

  document.body.classList.toggle("advanced", viewMode === "advanced");
  document.body.classList.toggle("simple", viewMode !== "advanced");
  displayModeInput.checked = viewMode === "advanced";
  displayModeWrap.classList.toggle("on", viewMode === "advanced");

  cookiesButton.classList.toggle("on", mode === "always");
  cookiesButton.textContent = `Cookies ${mode}`;

  const missing = [];
  if (!settings.ytDlpPath) {
    missing.push("yt-dlp");
  }
  if (!settings.ffmpegPath) {
    missing.push("ffmpeg");
  }
  if (!settings.downloadDir) {
    missing.push("download folder");
  }

  const setup = missing.length ? `Missing: ${missing.join(", ")}.` : "Ready.";
  const cookies = viewMode === "advanced"
    ? (mode === "auto"
      ? "Cookies are auto; public videos try without cookies first."
      : mode === "never"
        ? "Cookies are never used."
        : "Cookies are always used; public downloads may fail while Chrome is open.")
    : `Cookies ${mode}.`;
  settingsStatus.textContent = `${setup} ${cookies}`;
}

function normalizedCookieMode(settings) {
  return ["auto", "never", "always"].includes(settings.cookieMode) ? settings.cookieMode : "auto";
}

function displayMode(settings) {
  return settings.displayMode === "advanced" ? "advanced" : "simple";
}

function summaryText(job) {
  if (job.finalPath) {
    const suffix = job.status === "complete" && Number(job.exitCode || 0) !== 0
      ? "Saved. yt-dlp reported a warning."
      : "File";
    return `${suffix}: ${basename(job.finalPath)}`;
  }

  const lines = (job.output || []).map((entry) => entry.line || "").filter(Boolean);
  const destination = lastMatching(lines, /Destination:\s+(.+)$/i);
  if (destination) {
    const suffix = job.status === "complete" && Number(job.exitCode || 0) !== 0
      ? "Saved. yt-dlp reported a warning."
      : "File";
    return `${suffix}: ${basename(destination)}`;
  }

  const progress = [...lines].reverse().find((line) => line.includes("[download]") && line.includes("%"));
  if (progress) {
    return progress.replace(/^\[download\]\s*/i, "").trim();
  }

  const hint = [...(job.output || [])].reverse().find((entry) => entry.stream === "hint");
  if (hint?.line) {
    return hint.line;
  }

  if (job.status === "complete") {
    return "Done.";
  }

  if (job.status === "failed") {
    const error = [...(job.output || [])].reverse().find((entry) => entry.stream === "error" || entry.stream === "stderr");
    return error?.line || "Failed.";
  }

  return "Starting...";
}

function lastMatching(lines, pattern) {
  for (const line of [...lines].reverse()) {
    const match = line.match(pattern);
    if (match) {
      return match[1];
    }
  }
  return "";
}

function basename(path) {
  return String(path)
    .replace(/^\\\\\?\\/, "")
    .split(/[\\/]/)
    .filter(Boolean)
    .pop() || String(path);
}
