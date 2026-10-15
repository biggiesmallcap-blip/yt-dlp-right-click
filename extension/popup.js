import {
  ACTIVE_STATUSES,
  DEFAULT_SETTINGS,
  GROUP_TITLES,
  PRESETS,
  basename,
  formatBytes,
  formatDuration,
  isHttpUrl,
  missingSettings,
  normalizeSettings,
  presetById,
  siteLabel,
  stripExtension,
  timeAgo
} from "./shared.js";

const SEGMENTS = 24;

const $ = (id) => document.getElementById(id);
const jobsContainer = $("jobs");
const template = $("job-template");
const presetSelect = $("preset");
const downloadButton = $("download");
const cookieSelect = $("cookie-mode");
const logsToggle = $("toggle-logs");
const noticesBox = $("notices");
const tabTarget = $("tab-target");

let settings = normalizeSettings({});
let jobs = [];
let progressById = {};
let lastUpdateResult = null;
let activeTab = null;
let hostInfo = null;
let extensionUpdate = null;
let flash = null;
const jobNodes = new Map();

init().catch((error) => setFlash(error.message || String(error)));

async function init() {
  $("version").textContent = `v${chrome.runtime.getManifest().version}`;
  renderPresetOptions();

  const stored = await chrome.storage.local.get({
    jobs: [],
    jobProgress: {},
    settings: DEFAULT_SETTINGS,
    extensionUpdate: null,
    lastYtDlpUpdateResult: null
  });
  settings = normalizeSettings(stored.settings);
  jobs = Array.isArray(stored.jobs) ? stored.jobs : [];
  progressById = stored.jobProgress || {};
  extensionUpdate = stored.extensionUpdate;
  lastUpdateResult = stored.lastYtDlpUpdateResult;

  applySettings();
  renderJobs();
  activeTab = await currentTab();
  renderTabTarget();
  renderNotices();

  chrome.runtime.sendMessage({ type: "popupOpened" }).catch(() => {});
  chrome.runtime.sendMessage({ type: "getHostInfo" }).then((info) => {
    hostInfo = info;
    renderNotices();
  }).catch(() => {});

  setInterval(refreshTimes, 30_000);
}

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== "local") return;
  if (changes.jobs) {
    jobs = changes.jobs.newValue || [];
    renderJobs();
  }
  if (changes.jobProgress) {
    progressById = changes.jobProgress.newValue || {};
    // Only meters change on a progress tick; leave the rest of each row alone.
    for (const job of jobs) {
      const node = jobNodes.get(job.id);
      if (node) renderMeter(node.querySelector(".meter"), job);
    }
  }
  if (changes.lastYtDlpUpdateResult) {
    lastUpdateResult = changes.lastYtDlpUpdateResult.newValue;
    renderNotices();
  }
  if (changes.settings) {
    settings = normalizeSettings(changes.settings.newValue);
    applySettings();
    renderNotices();
    renderJobs();
  }
  if (changes.extensionUpdate) {
    extensionUpdate = changes.extensionUpdate.newValue;
    renderNotices();
  }
});

// Controls ----------------------------------------------------------------

$("open-settings").addEventListener("click", () => chrome.runtime.openOptionsPage());
$("clear").addEventListener("click", () => chrome.runtime.sendMessage({ type: "clearCompletedJobs" }));
downloadButton.addEventListener("click", downloadThisTab);
presetSelect.addEventListener("change", () => saveSettings({ manualPreset: presetSelect.value }));
cookieSelect.addEventListener("change", () => saveSettings({ cookieMode: cookieSelect.value }));
logsToggle.addEventListener("click", () => {
  saveSettings({ displayMode: settings.displayMode === "advanced" ? "simple" : "advanced" });
});
jobsContainer.addEventListener("click", handleJobAction);
noticesBox.addEventListener("click", handleNoticeAction);

async function saveSettings(patch) {
  // The service worker merges settings under its storage lock, so this cannot
  // race with it remembering the last-used preset.
  const response = await chrome.runtime.sendMessage({ type: "saveSettings", patch });
  if (!response?.ok) setFlash(response?.message || "Could not save the setting.");
}

function renderPresetOptions() {
  presetSelect.textContent = "";
  for (const [group, title] of Object.entries(GROUP_TITLES)) {
    const optgroup = document.createElement("optgroup");
    optgroup.label = title;
    for (const preset of PRESETS.filter((item) => item.group === group)) {
      optgroup.append(new Option(preset.title, preset.id));
    }
    presetSelect.append(optgroup);
  }
}

function applySettings() {
  presetSelect.value = settings.manualPreset;
  cookieSelect.value = settings.cookieMode;
  cookieSelect.parentElement.classList.toggle("always", settings.cookieMode === "always");
  const logs = settings.displayMode === "advanced";
  document.body.classList.toggle("logs", logs);
  logsToggle.setAttribute("aria-pressed", String(logs));
  logsToggle.title = logs ? "Hide logs" : "Show full logs";
}

async function currentTab() {
  try {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    return tab || null;
  } catch {
    return null;
  }
}

function renderTabTarget() {
  tabTarget.textContent = "";
  const url = activeTab?.url || "";
  if (!isHttpUrl(url)) {
    tabTarget.textContent = "This tab has no web address to download. Right-click a link or video instead.";
    downloadButton.disabled = true;
    return;
  }
  downloadButton.disabled = false;
  const site = document.createElement("strong");
  site.textContent = siteLabel(url);
  tabTarget.append(site, ` ${activeTab.title || url}`);
  tabTarget.title = url;
}

async function downloadThisTab() {
  if (!activeTab?.url) return;
  downloadButton.disabled = true;
  try {
    const response = await chrome.runtime.sendMessage({
      type: "startDownload",
      presetId: presetSelect.value,
      url: activeTab.url
    });
    setFlash(response?.ok ? null : response?.message || "The download did not start.");
  } catch (error) {
    setFlash(error.message || String(error));
  } finally {
    downloadButton.disabled = false;
  }
}

// Notices ------------------------------------------------------------------

function setFlash(message) {
  flash = message;
  renderNotices();
}

function renderNotices() {
  const notices = [];
  if (flash) {
    notices.push({ tone: "bad", text: flash, action: "dismiss", label: "Dismiss" });
  }

  // The helper comes first: once it runs, it fills in the missing paths itself.
  const missing = missingSettings(settings, null);
  if (hostInfo && !hostInfo.ok) {
    notices.push({ tone: "bad", text: hostInfo.message, action: "settings", label: "Help" });
  } else if (missing.length && hostInfo) {
    notices.push({ tone: "warn", text: `Finish setup: add the ${missing.join(", ")}.`, action: "settings", label: "Open settings" });
  } else if (hostInfo?.outdated) {
    notices.push({
      tone: "warn",
      text: `The native host (v${hostInfo.hostVersion}) is older than the extension, so newer presets will fail. Rebuild or reinstall it.`,
      action: "settings",
      label: "How"
    });
  }

  if (lastUpdateResult && !lastUpdateResult.ok && lastUpdateResult.source !== "manual" && settings.autoUpdateYtDlp) {
    const lastLine = String(lastUpdateResult.output || "").trim().split("\n").pop() || "";
    notices.push({
      tone: "warn",
      text: `The automatic yt-dlp update failed${lastLine ? `: ${lastLine.slice(0, 160)}` : "."}`,
      action: "settings",
      label: "Details"
    });
  }

  if (extensionUpdate?.available) {
    notices.push({
      tone: "good",
      text: `Version ${extensionUpdate.latestVersion} of yt-dlp Right Click is out.`,
      action: "release",
      label: "Get it"
    });
  }

  noticesBox.textContent = "";
  for (const notice of notices) {
    const box = document.createElement("div");
    box.className = `notice ${notice.tone}`;
    const text = document.createElement("p");
    text.textContent = notice.text;
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.action = notice.action;
    button.textContent = notice.label;
    box.append(text, button);
    noticesBox.append(box);
  }
}

function handleNoticeAction(event) {
  const action = event.target.closest("button[data-action]")?.dataset.action;
  if (action === "dismiss") setFlash(null);
  if (action === "settings") chrome.runtime.openOptionsPage();
  if (action === "release" && extensionUpdate?.url) chrome.tabs.create({ url: extensionUpdate.url });
}

// Jobs ---------------------------------------------------------------------

function renderJobs() {
  const seen = new Set();

  if (!jobs.length) {
    jobNodes.clear();
    jobsContainer.textContent = "";
    const empty = document.createElement("div");
    empty.className = "empty";
    const strong = document.createElement("strong");
    strong.textContent = "Nothing downloaded yet";
    empty.append(strong, "Right-click a video, link, or page and choose yt-dlp, or use Download above.");
    jobsContainer.append(empty);
    return;
  }
  jobsContainer.querySelector(".empty")?.remove();

  jobs.forEach((job, index) => {
    seen.add(job.id);
    let node = jobNodes.get(job.id);
    if (!node) {
      node = template.content.firstElementChild.cloneNode(true);
      const segments = node.querySelector(".segments");
      for (let i = 0; i < SEGMENTS; i += 1) {
        const segment = document.createElement("span");
        segment.style.setProperty("--i", String(i));
        segments.append(segment);
      }
      jobNodes.set(job.id, node);
    }
    updateJobNode(node, job);
    if (jobsContainer.children[index] !== node) {
      jobsContainer.insertBefore(node, jobsContainer.children[index] || null);
    }
  });

  for (const [id, node] of jobNodes) {
    if (!seen.has(id)) {
      node.remove();
      jobNodes.delete(id);
    }
  }
}

function updateJobNode(node, job) {
  node.className = `job ${job.status || "unknown"}`;
  node.dataset.jobId = job.id;

  node.querySelector(".job-title").textContent = jobTitle(job);
  node.querySelector(".job-title").title = job.url || "";
  const time = node.querySelector(".job-time");
  time.dateTime = job.startedAt || "";
  time.textContent = timeAgo(job.startedAt);
  time.title = job.startedAt ? new Date(job.startedAt).toLocaleString() : "";

  node.querySelector(".job-preset").textContent = job.preset === "local_error" ? "" : job.presetTitle || "";
  const site = job.url ? siteLabel(job.url) : "";
  node.querySelector(".job-site").textContent = jobTitle(job) === site ? "" : site;

  renderMeter(node.querySelector(".meter"), job);
  node.querySelector(".job-summary").textContent = summaryText(job);

  // Progress arrives twice a second. Rebuilding buttons that often would
  // swallow clicks (mousedown and mouseup landing on different nodes), so only
  // re-render actions and logs when what they show actually changed.
  const actionsKey = [job.status, job.finalPath, job.finalPathKind, job.url].join("|");
  if (node.dataset.actionsKey !== actionsKey) {
    node.dataset.actionsKey = actionsKey;
    renderActions(node.querySelector(".job-actions"), job);
  }
  const output = job.output || [];
  const last = output.at(-1);
  const logKey = [output.length, last?.at, last?.line, job.pid, job.exitCode, job.logPath, job.cookiesUsed].join("|");
  if (node.dataset.logKey !== logKey) {
    node.dataset.logKey = logKey;
    renderLog(node.querySelector(".job-log"), job);
  }
}

function jobTitle(job) {
  if (job.preset === "local_error") return job.presetTitle || "Error";
  if (job.title) return job.title;
  if (job.finalPath) return stripExtension(basename(job.finalPath));
  return job.url ? siteLabel(job.url) || job.url : job.presetTitle || "Download";
}

function renderMeter(meter, job) {
  const active = ACTIVE_STATUSES.has(job.status);
  meter.hidden = !active;
  if (!active) return;

  const live = progressById[job.id];
  const progress = live || {};
  const percent = Number.isFinite(progress.percent) ? progress.percent : null;
  const segments = meter.querySelector(".segments");
  const lit = percent === null ? 0 : Math.round((percent / 100) * SEGMENTS);
  segments.classList.toggle("indeterminate", percent === null);
  [...segments.children].forEach((segment, index) => segment.classList.toggle("on", index < lit));
  if (percent === null) {
    segments.removeAttribute("aria-valuenow");
  } else {
    segments.setAttribute("aria-valuenow", String(Math.round(percent)));
  }

  const left = [];
  const right = [];
  if (job.status === "starting" || !live) {
    left.push("Starting");
  } else if (progress.status === "finished") {
    left.push("Converting and saving");
  } else if (percent !== null) {
    left.push(`${percent.toFixed(percent < 10 ? 1 : 0)}%`);
    const total = formatBytes(progress.totalBytes);
    if (total) left.push(`of ${total}`);
  }
  if (progress.status !== "finished") {
    const speed = formatBytes(progress.speed);
    if (speed) right.push(`${speed}/s`);
    const eta = formatDuration(progress.eta);
    if (eta && progress.eta > 0) right.push(`${eta} left`);
  }

  const text = meter.querySelector(".meter-text");
  text.textContent = "";
  const leftSpan = document.createElement("span");
  const item = progress.itemCount > 1 ? `Item ${progress.item || "?"} of ${progress.itemCount}: ` : "";
  leftSpan.textContent = item + left.join(" ");
  const rightSpan = document.createElement("span");
  rightSpan.textContent = right.join(", ");
  text.append(leftSpan, rightSpan);
}

function summaryText(job) {
  const output = job.output || [];
  if (job.status === "complete") {
    const warning = Number(job.exitCode || 0) !== 0 ? " yt-dlp reported warnings; check the log." : "";
    if (job.finalPathKind === "folder") {
      if (!job.fileCount) {
        const hint = [...output].reverse().find((entry) => entry.stream === "hint");
        return `${hint?.line || "Nothing new to download."}${warning}`;
      }
      return `Saved ${job.fileCount} file${job.fileCount === 1 ? "" : "s"} to ${basename(job.finalPath)}.${warning}`;
    }
    return job.finalPath ? `Saved as ${basename(job.finalPath)}.${warning}` : `Finished.${warning}`;
  }
  if (job.status === "cancelled") return "Cancelled. Resume picks up from the partial file.";
  if (job.status === "failed") {
    const reversed = [...output].reverse();
    const error = reversed.find((entry) => entry.stream === "error" || /^ERROR:/i.test(entry.line || ""))
      || reversed.find((entry) => entry.stream === "stderr");
    const hint = reversed.find((entry) => entry.stream === "hint" && /fix|cookies\.txt|settings/i.test(entry.line || ""));
    const message = (error?.line || "Failed. Open the log for details.").replace(/^ERROR:\s*/i, "");
    return hint && hint !== error ? `${message}\n${hint.line}` : message;
  }
  return "";
}

function renderActions(container, job) {
  container.textContent = "";
  const add = (label, action, className = "") => {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    button.dataset.action = action;
    if (className) button.className = className;
    container.append(button);
  };

  if (ACTIVE_STATUSES.has(job.status)) {
    add("Cancel", "cancel");
    return;
  }
  if (job.status === "complete") {
    add(job.finalPathKind === "folder" || !job.finalPath ? "Open folder" : "Show in folder", "open");
  }
  if ((job.status === "failed" || job.status === "cancelled") && job.url && presetById(job.preset)) {
    add(job.status === "cancelled" ? "Resume" : "Retry", "retry");
  }
  if (job.url) add("Copy link", "copy");
  add("Remove", "remove", "remove");
}

function renderLog(details, job) {
  const meta = details.querySelector(".log-meta");
  meta.textContent = "";
  const parts = [];
  if (job.pid) parts.push(`pid ${job.pid}`);
  if (job.exitCode !== undefined && job.exitCode !== null) parts.push(`exit ${job.exitCode}`);
  if (job.cookiesUsed) parts.push("used cookies");
  meta.append(parts.join(", "));
  if (job.logPath) {
    const open = document.createElement("button");
    open.type = "button";
    open.className = "text-button";
    open.dataset.action = "log";
    open.textContent = "Show log file";
    meta.append(" ", open);
  }

  const pre = details.querySelector(".log-lines");
  const atBottom = pre.scrollHeight - pre.scrollTop - pre.clientHeight < 8;
  pre.textContent = "";
  const lines = (job.output || []).slice(-80);
  if (!lines.length) {
    pre.textContent = "No output yet.";
    return;
  }
  for (const entry of lines) {
    const line = document.createElement("div");
    if (entry.stream === "error" || /^ERROR:/i.test(entry.line || "")) line.className = "err";
    if (entry.stream === "hint") line.className = "hint";
    line.textContent = entry.line || "";
    pre.append(line);
  }
  if (atBottom) pre.scrollTop = pre.scrollHeight;
}

async function handleJobAction(event) {
  const button = event.target.closest("button[data-action]");
  const jobId = event.target.closest(".job")?.dataset.jobId;
  if (!button || !jobId) return;

  const job = jobs.find((item) => item.id === jobId);
  const action = button.dataset.action;
  if (action === "copy") {
    try {
      await navigator.clipboard.writeText(job?.url || "");
      button.textContent = "Copied";
      setTimeout(() => { button.textContent = "Copy link"; }, 1200);
    } catch (error) {
      setFlash(`Could not copy: ${error.message || error}`);
    }
    return;
  }

  const types = {
    cancel: "cancelJob",
    open: "openJobFolder",
    log: "openJobLog",
    retry: "retryJob",
    remove: "removeJob"
  };
  if (!types[action]) return;

  button.disabled = true;
  try {
    const response = await chrome.runtime.sendMessage({ type: types[action], jobId });
    if (!response?.ok) setFlash(response?.message || `${button.textContent} failed.`);
  } catch (error) {
    setFlash(error.message || String(error));
  } finally {
    button.disabled = false;
  }
}

function refreshTimes() {
  for (const node of jobNodes.values()) {
    const time = node.querySelector(".job-time");
    time.textContent = timeAgo(time.dateTime);
  }
}
