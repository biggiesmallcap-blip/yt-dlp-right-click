import {
  ACTIVE_STATUSES,
  DEFAULT_SETTINGS,
  GROUP_TITLES,
  HOST_NAME,
  MAX_JOBS,
  MAX_OUTPUT_LINES,
  PRESETS,
  REPO,
  RELEASES_URL,
  basename,
  compareVersions,
  extractTargetUrl,
  hostSettings,
  isHttpUrl,
  missingSettings,
  normalizeSettings,
  presetById
} from "./shared.js";

const MAINTENANCE_ALARM = "maintenance";
const YT_DLP_UPDATE_INTERVAL_MS = 24 * 60 * 60 * 1000;
const YT_DLP_UPDATE_RETRY_MS = 6 * 60 * 60 * 1000;
const EXTENSION_UPDATE_INTERVAL_MS = 24 * 60 * 60 * 1000;
const HOST_INFO_TTL_MS = 10 * 60 * 1000;
const INSTAGRAM_PATTERNS = ["*://instagram.com/*", "*://*.instagram.com/*"];
const EXTENSION_VERSION = chrome.runtime.getManifest().version;

/** jobId -> native messaging Port for jobs started by this worker instance. */
const ports = new Map();
let hostInfoCache = null;

/**
 * Live progress lives in its own small storage key instead of inside `jobs`,
 * so twice-a-second updates do not rewrite every job's full output history.
 * Only this worker writes it, and only jobs it started can have progress.
 */
const progressById = {};

function setProgress(jobId, progress) {
  if (progress) {
    progressById[jobId] = progress;
  } else {
    delete progressById[jobId];
  }
  return chrome.storage.local.set({ jobProgress: { ...progressById } });
}

// ---------------------------------------------------------------------------
// Storage. Every job mutation goes through one queue so concurrent native
// messages cannot read stale state and overwrite each other.

let storageQueue = Promise.resolve();

function withStorageLock(task) {
  const run = storageQueue.then(task, task);
  storageQueue = run.catch(() => {});
  return run;
}

async function readJobs() {
  const { jobs } = await chrome.storage.local.get({ jobs: [] });
  return Array.isArray(jobs) ? jobs : [];
}

function mutateJobs(mutator) {
  return withStorageLock(async () => {
    const jobs = await readJobs();
    const next = (await mutator(jobs)) || jobs;
    await chrome.storage.local.set({ jobs: next.slice(0, MAX_JOBS) });
    return next;
  });
}

function patchJob(jobId, patch) {
  return mutateJobs((jobs) => jobs.map((job) => (
    job.id === jobId ? { ...job, ...(typeof patch === "function" ? patch(job) : patch) } : job
  )));
}

/** Like patchJob, but leaves jobs that already finished, failed, or were cancelled alone. */
function patchActiveJob(jobId, patch) {
  return mutateJobs((jobs) => jobs.map((job) => (
    job.id === jobId && ACTIVE_STATUSES.has(job.status) ? { ...job, ...patch } : job
  )));
}

function appendJobOutput(jobId, line, stream) {
  return patchJob(jobId, (job) => ({
    output: [...(job.output || []), { stream, line, at: new Date().toISOString() }].slice(-MAX_OUTPUT_LINES)
  }));
}

async function getSettings() {
  const { settings } = await chrome.storage.local.get({ settings: DEFAULT_SETTINGS });
  return normalizeSettings(settings);
}

function updateSettings(patch) {
  return withStorageLock(async () => {
    const settings = await getSettings();
    const next = normalizeSettings({ ...settings, ...patch });
    await chrome.storage.local.set({ settings: next });
    return next;
  });
}

async function migrateSettings() {
  const { settings } = await chrome.storage.local.get({ settings: null });
  await chrome.storage.local.set({ settings: normalizeSettings(settings || {}) });
}

// ---------------------------------------------------------------------------
// Lifecycle

chrome.runtime.onInstalled.addListener(async (details) => {
  await migrateSettings();
  await createContextMenus();
  await ensureAlarm();
  await updateBadge();
  // Fills yt-dlp/ffmpeg/folder from the helper before anything else runs.
  await getHostInfo(true).catch(() => {});
  if (details.reason === "install") {
    chrome.runtime.openOptionsPage();
  }
  runMaintenance("installed");
});

chrome.runtime.onStartup.addListener(async () => {
  await ensureAlarm();
  await updateBadge();
  runMaintenance("startup");
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === MAINTENANCE_ALARM) {
    runMaintenance("alarm");
  }
});

// A fresh worker has no open ports, so any job still marked active belongs to
// a previous worker instance or browser session and can no longer report back.
markOrphanedJobs().then(updateBadge).catch(() => {});

async function ensureAlarm() {
  const existing = await chrome.alarms.get(MAINTENANCE_ALARM);
  if (!existing) {
    await chrome.alarms.create(MAINTENANCE_ALARM, { delayInMinutes: 1, periodInMinutes: 60 });
  }
}

function runMaintenance(source) {
  maybeAutoUpdateYtDlp(source).catch(() => {});
  maybeCheckExtensionUpdate(false).catch(() => {});
}

async function markOrphanedJobs() {
  const now = new Date().toISOString();
  await chrome.storage.local.set({ jobProgress: {} });
  await mutateJobs((jobs) => jobs.map((job) => {
    if (!ACTIVE_STATUSES.has(job.status) || ports.has(job.id)) {
      return job;
    }
    // The native host stops yt-dlp when its port closes, so the download
    // ended here; the partial file is kept and Retry resumes it.
    return {
      ...job,
      status: "failed",
      finishedAt: now,
      output: [
        ...(job.output || []),
        {
          stream: "error",
          line: "Stopped because the browser or extension restarted. Retry resumes from the partial file.",
          at: now
        }
      ].slice(-MAX_OUTPUT_LINES)
    };
  }));
}

// ---------------------------------------------------------------------------
// Context menu

const MENU_CONTEXTS = ["page", "link", "selection", "image", "video", "audio", "frame"];

async function createContextMenus() {
  await chrome.contextMenus.removeAll();
  const settings = await getSettings();

  chrome.contextMenus.create({ id: "root", title: "yt-dlp", contexts: MENU_CONTEXTS });
  chrome.contextMenus.create({
    id: "repeat",
    parentId: "root",
    title: repeatMenuTitle(settings.manualPreset),
    contexts: MENU_CONTEXTS
  });
  chrome.contextMenus.create({ id: "sep", parentId: "root", type: "separator", contexts: MENU_CONTEXTS });

  for (const [group, title] of Object.entries(GROUP_TITLES)) {
    chrome.contextMenus.create({
      id: `group_${group}`,
      parentId: "root",
      title,
      contexts: MENU_CONTEXTS,
      ...(group === "instagram" ? { documentUrlPatterns: INSTAGRAM_PATTERNS } : {})
    });
  }

  for (const preset of PRESETS) {
    chrome.contextMenus.create({
      id: preset.id,
      parentId: `group_${preset.group}`,
      title: preset.title,
      contexts: MENU_CONTEXTS
    });
  }
}

function repeatMenuTitle(presetId) {
  const preset = presetById(presetId) || PRESETS[0];
  return `Download again as ${GROUP_TITLES[preset.group]}: ${preset.title}`;
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  handleContextMenuClick(info, tab).catch((error) => {
    recordLocalError("Menu action failed", errorText(error));
  });
});

async function handleContextMenuClick(info, tab) {
  const presetId = info.menuItemId === "repeat"
    ? (await getSettings()).manualPreset
    : info.menuItemId;
  if (!presetById(presetId)) {
    return;
  }

  const url = extractTargetUrl(info, tab);
  if (!url) {
    await recordLocalError("Nothing to download here", "Right-click a page, link, video, or a selected http(s) URL.");
    return;
  }

  const response = await startDownload(presetId, url);
  if (!response.ok) {
    await recordLocalError("Download did not start", response.message || "Could not start download.");
  }
}

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName !== "local" || !changes.settings) {
    return;
  }
  const before = changes.settings.oldValue || {};
  const after = changes.settings.newValue || {};
  if (before.manualPreset !== after.manualPreset) {
    chrome.contextMenus.update("repeat", { title: repeatMenuTitle(after.manualPreset) }, () => void chrome.runtime.lastError);
  }
  if (after.autoUpdateYtDlp && !before.autoUpdateYtDlp) {
    maybeAutoUpdateYtDlp("settings").catch(() => {});
  }
});

// ---------------------------------------------------------------------------
// Messages from the popup and options page

const MESSAGE_HANDLERS = {
  startDownload: (message) => startDownload(message.presetId, message.url),
  retryJob: (message) => retryJob(message.jobId),
  cancelJob: (message) => cancelJob(message.jobId),
  openJobFolder: (message) => openJobPath(message.jobId, "final"),
  openJobLog: (message) => openJobPath(message.jobId, "log"),
  removeJob: (message) => removeJob(message.jobId),
  clearCompletedJobs: () => clearCompletedJobs(),
  updateYtDlp: () => runYtDlpUpdate("manual"),
  getHostInfo: (message) => getHostInfo(Boolean(message.force)),
  validateSettings: () => validateSettings(),
  saveSettings: async (message) => ({ ok: true, settings: await updateSettings(message.patch || {}) }),
  checkExtensionUpdate: () => maybeCheckExtensionUpdate(true),
  popupOpened: () => markFailuresSeen()
};

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  // Only this extension's own pages may drive local downloads.
  if (sender.id !== chrome.runtime.id || !String(sender.url || "").startsWith(chrome.runtime.getURL(""))) {
    return false;
  }
  const handler = MESSAGE_HANDLERS[message?.type];
  if (!handler) {
    return false;
  }
  Promise.resolve()
    .then(() => handler(message))
    .then((response) => sendResponse(response ?? { ok: true }))
    .catch((error) => sendResponse({ ok: false, message: errorText(error) }));
  return true;
});

// ---------------------------------------------------------------------------
// Jobs

async function startDownload(presetId, url) {
  const preset = presetById(presetId);
  if (!preset) {
    return { ok: false, message: "Unknown download preset." };
  }
  if (!isHttpUrl(url)) {
    return { ok: false, message: "This tab has no http(s) address to download." };
  }

  const settings = await getSettings();
  const missing = missingSettings(settings, preset);
  if (missing.length > 0) {
    return { ok: false, message: `Set the ${missing.join(", ")} in Settings first.` };
  }

  if (settings.manualPreset !== preset.id) {
    await updateSettings({ manualPreset: preset.id });
  }
  await startNativeJob(preset, url, settings);
  return { ok: true };
}

async function startNativeJob(preset, url, settings) {
  const jobId = crypto.randomUUID();
  const now = new Date().toISOString();
  await mutateJobs((jobs) => [{
    id: jobId,
    preset: preset.id,
    presetTitle: preset.title,
    group: preset.group,
    url,
    title: "",
    status: "starting",
    output: [],
    logPath: "",
    finalPath: "",
    startedAt: now,
    finishedAt: ""
  }, ...jobs]);
  await updateBadge();

  if (ytDlpUpdateInFlight) {
    await appendJobOutput(jobId, "Waiting for the yt-dlp update to finish...", "hint");
    await ytDlpUpdateInFlight.catch(() => {});
    // The user may have cancelled or removed the job while it waited.
    const job = (await readJobs()).find((item) => item.id === jobId);
    if (!job || !ACTIVE_STATUSES.has(job.status)) {
      return;
    }
  }

  let port;
  try {
    port = chrome.runtime.connectNative(HOST_NAME);
  } catch (error) {
    await finishJobWithError(jobId, nativeHostErrorMessage(errorText(error)));
    return;
  }
  ports.set(jobId, port);

  port.onMessage.addListener((message) => {
    handleNativeMessage(jobId, message).catch((error) => {
      appendJobOutput(jobId, `Could not record host message: ${errorText(error)}`, "error");
    });
  });

  port.onDisconnect.addListener(() => {
    const reason = chrome.runtime.lastError?.message || "";
    ports.delete(jobId);
    handleNativeDisconnect(jobId, reason).catch(() => {});
  });

  port.postMessage({ type: "start", jobId, preset: preset.id, url, settings: hostSettings(settings) });
}

async function handleNativeMessage(jobId, message) {
  if (!message || typeof message.type !== "string") {
    return;
  }

  switch (message.type) {
    case "started":
      await patchActiveJob(jobId, {
        status: "running",
        pid: Number(message.pid) || 0,
        logPath: String(message.logPath || ""),
        cookiesUsed: Boolean(message.cookiesUsed)
      });
      return;
    case "meta":
      await patchActiveJob(jobId, { title: String(message.title || "").slice(0, 300) });
      return;
    case "progress":
      if (ports.has(jobId)) {
        await setProgress(jobId, sanitizeProgress(message));
      }
      return;
    case "output":
      await appendJobOutput(jobId, String(message.line || ""), String(message.stream || "stdout"));
      return;
    case "finished":
      await handleFinished(jobId, message);
      return;
    case "error":
      await finishJobWithError(jobId, explainHostError(String(message.message || "Unknown native host error")));
      return;
    default:
  }
}

function sanitizeProgress(message) {
  const number = (value) => (Number.isFinite(Number(value)) && value !== null ? Number(value) : null);
  return {
    percent: number(message.percent),
    downloadedBytes: number(message.downloadedBytes),
    totalBytes: number(message.totalBytes),
    speed: number(message.speed),
    eta: number(message.eta),
    status: String(message.status || "").slice(0, 32),
    item: number(message.item),
    itemCount: number(message.itemCount)
  };
}

async function handleFinished(jobId, message) {
  const status = message.cancelled ? "cancelled" : message.success ? "complete" : "failed";
  await setProgress(jobId, null);
  const jobs = await patchJob(jobId, {
    status,
    exitCode: Number.isFinite(Number(message.exitCode)) ? Number(message.exitCode) : null,
    finishedAt: new Date().toISOString(),
    logPath: String(message.logPath || ""),
    finalPath: String(message.finalPath || ""),
    finalPathKind: message.finalPathKind === "folder" ? "folder" : "file",
    fileCount: Number(message.fileCount) || 0
  });
  if (status === "failed") {
    await bumpUnseenFailures();
  }
  await updateBadge();
  const job = jobs.find((item) => item.id === jobId);
  if (job && status !== "cancelled") {
    notifyFinished(job).catch(() => {});
  }
}

function handleNativeDisconnect(jobId, reason) {
  // The host exits right after sending "finished", so this often fires while
  // that update is still queued. finishJobWithError re-checks the status
  // inside the storage lock and leaves finished jobs alone.
  return finishJobWithError(
    jobId,
    reason ? nativeHostErrorMessage(reason) : "The native host stopped without reporting a result. Check the job log."
  );
}

/** Marks a still-active job as failed. No-op if it already finished. */
async function finishJobWithError(jobId, message) {
  const now = new Date().toISOString();
  let failed = null;
  await mutateJobs((jobs) => jobs.map((job) => {
    if (job.id !== jobId || !ACTIVE_STATUSES.has(job.status)) {
      return job;
    }
    failed = {
      ...job,
      status: "failed",
      finishedAt: now,
      output: [...(job.output || []), { stream: "error", line: message, at: now }].slice(-MAX_OUTPUT_LINES)
    };
    return failed;
  }));
  if (!failed) {
    return;
  }
  await setProgress(jobId, null);
  await bumpUnseenFailures();
  await updateBadge();
  notifyFinished(failed).catch(() => {});
}

function nativeHostErrorMessage(message) {
  const lower = String(message).toLowerCase();
  if (lower.includes("specified native messaging host not found")) {
    return "The helper program isn't installed yet. Run Setup.cmd from the yt-dlp Right Click download.";
  }
  if (lower.includes("access to the specified native messaging host is forbidden")) {
    return "The helper program was set up for an older copy of the extension. Run Setup.cmd from the latest yt-dlp Right Click download.";
  }
  if (lower.includes("native host has exited")) {
    return "The helper program stopped unexpectedly. Run Setup.cmd again, then Check setup in Settings.";
  }
  return `Native host disconnected: ${message}`;
}

function explainHostError(message) {
  if (/unsupported (preset|message type)/i.test(message)) {
    return `${message}\nThe installed native host is older than this extension. Rebuild it (cargo build --release) or install the latest native host release.`;
  }
  return message;
}

async function recordLocalError(title, detail) {
  const now = new Date().toISOString();
  await mutateJobs((jobs) => [{
    id: crypto.randomUUID(),
    preset: "local_error",
    presetTitle: title,
    url: "",
    title,
    status: "failed",
    output: [{ stream: "error", line: detail, at: now }],
    logPath: "",
    finalPath: "",
    startedAt: now,
    finishedAt: now
  }, ...jobs]);
  await bumpUnseenFailures();
  await updateBadge();
}

async function retryJob(jobId) {
  const job = (await readJobs()).find((item) => item.id === jobId);
  if (!job) {
    return { ok: false, message: "That job is no longer in the list." };
  }
  if (ACTIVE_STATUSES.has(job.status)) {
    return { ok: false, message: "That job is still running." };
  }
  if (!job.url || !presetById(job.preset)) {
    return { ok: false, message: "This entry has no download to retry." };
  }
  const response = await startDownload(job.preset, job.url);
  if (response.ok) {
    await mutateJobs((jobs) => jobs.filter((item) => item.id !== jobId));
  }
  return response;
}

async function cancelJob(jobId) {
  const port = ports.get(jobId);
  if (!port) {
    const job = (await readJobs()).find((item) => item.id === jobId);
    if (job && ACTIVE_STATUSES.has(job.status)) {
      await finishJobWithError(jobId, "Cancelled, but the download had already lost contact with the extension.");
    }
    return { ok: true };
  }
  await appendJobOutput(jobId, "Cancelling...", "hint");
  port.postMessage({ type: "cancel" });
  return { ok: true };
}

async function removeJob(jobId) {
  await mutateJobs((jobs) => jobs.filter((job) => job.id !== jobId || ACTIVE_STATUSES.has(job.status)));
  await updateBadge();
  return { ok: true };
}

async function clearCompletedJobs() {
  await mutateJobs((jobs) => jobs.filter((job) => ACTIVE_STATUSES.has(job.status)));
  await chrome.storage.local.set({ unseenFailures: 0 });
  await updateBadge();
  return { ok: true };
}

async function openJobPath(jobId, which) {
  const settings = await getSettings();
  const job = (await readJobs()).find((item) => item.id === jobId);
  const path = which === "log" ? job?.logPath : (job?.finalPath || settings.downloadDir);
  if (!path) {
    return { ok: false, message: which === "log" ? "This job has no log file." : "Set a download folder in Settings first." };
  }
  const response = await sendNativeMessage({ type: "openPath", path, settings: hostSettings(settings) });
  return { ok: Boolean(response?.ok), message: response?.message || "" };
}

// ---------------------------------------------------------------------------
// Badge and notifications

async function bumpUnseenFailures() {
  await withStorageLock(async () => {
    const { unseenFailures } = await chrome.storage.local.get({ unseenFailures: 0 });
    await chrome.storage.local.set({ unseenFailures: Number(unseenFailures || 0) + 1 });
  });
}

async function markFailuresSeen() {
  await chrome.storage.local.set({ unseenFailures: 0 });
  await updateBadge();
  return { ok: true };
}

async function updateBadge() {
  const jobs = await readJobs();
  const { unseenFailures } = await chrome.storage.local.get({ unseenFailures: 0 });
  const running = jobs.filter((job) => ACTIVE_STATUSES.has(job.status)).length;
  if (running) {
    await chrome.action.setBadgeText({ text: String(running) });
    await chrome.action.setBadgeBackgroundColor({ color: "#2563eb" });
  } else if (unseenFailures > 0) {
    await chrome.action.setBadgeText({ text: "!" });
    await chrome.action.setBadgeBackgroundColor({ color: "#dc2626" });
  } else {
    await chrome.action.setBadgeText({ text: "" });
  }
}

async function notifyFinished(job) {
  const settings = await getSettings();
  if (!settings.notifyOnFinish || job.preset === "local_error") {
    return;
  }
  const ok = job.status === "complete";
  const name = job.finalPath ? basename(job.finalPath) : job.title || job.url;
  const lastError = [...(job.output || [])].reverse().find((entry) => entry.stream === "error" || entry.stream === "stderr");
  const many = job.fileCount > 1 ? ` (${job.fileCount} files)` : "";
  chrome.notifications.create(`job:${job.id}`, {
    type: "basic",
    iconUrl: "icons/icon128.png",
    title: ok ? `Saved${many}` : "Download failed",
    message: ok ? name : (lastError?.line || name || "Open the popup for details.").slice(0, 200),
    contextMessage: job.presetTitle || "",
    priority: 0
  });
}

chrome.notifications.onClicked.addListener((notificationId) => {
  if (notificationId.startsWith("job:")) {
    openJobPath(notificationId.slice(4), "final").catch(() => {});
    chrome.notifications.clear(notificationId);
  }
});

// ---------------------------------------------------------------------------
// Native host helpers and updates

function sendNativeMessage(message) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendNativeMessage(HOST_NAME, message, (response) => {
      const error = chrome.runtime.lastError;
      if (error) {
        reject(new Error(nativeHostErrorMessage(error.message)));
        return;
      }
      resolve(response);
    });
  });
}

async function getHostInfo(force) {
  if (!force && hostInfoCache && Date.now() - hostInfoCache.at < HOST_INFO_TTL_MS) {
    return hostInfoCache.info;
  }
  let info;
  try {
    const response = await sendNativeMessage({ type: "version" });
    info = response?.type === "version"
      ? { ok: true, hostVersion: String(response.hostVersion || ""), presets: response.presets || [] }
      : { ok: true, hostVersion: "1.0.0", presets: [], outdated: true };
    if (response?.defaults) {
      await fillEmptySettings(response.defaults);
    }
  } catch (error) {
    info = { ok: false, message: errorText(error) };
  }
  if (info.ok) {
    const unsupported = PRESETS.filter((preset) => !info.presets.includes(preset.id)).map((preset) => preset.id);
    info.outdated = info.outdated || compareVersions(info.hostVersion, EXTENSION_VERSION) < 0 || unsupported.length > 0;
    info.extensionVersion = EXTENSION_VERSION;
  }
  // Never cache failures: the user is probably about to fix the install.
  hostInfoCache = info.ok ? { at: Date.now(), info } : null;
  return info;
}

/**
 * Copies the helper's suggested paths into settings the user has left empty.
 * Never overwrites a value the user typed.
 */
async function fillEmptySettings(defaults) {
  const keys = ["ytDlpPath", "ffmpegPath", "downloadDir"];
  await withStorageLock(async () => {
    const settings = await getSettings();
    const patch = {};
    for (const key of keys) {
      const value = typeof defaults[key] === "string" ? defaults[key].trim() : "";
      if (!settings[key] && value) {
        patch[key] = value;
      }
    }
    if (Object.keys(patch).length) {
      await chrome.storage.local.set({ settings: normalizeSettings({ ...settings, ...patch }) });
    }
  });
}

async function validateSettings() {
  const settings = await getSettings();
  const response = await sendNativeMessage({ type: "validate", settings: hostSettings(settings) });
  hostInfoCache = null;
  return { ...response, ok: Boolean(response?.ok), responded: true };
}

async function activeJobCount() {
  return (await readJobs()).filter((job) => ACTIVE_STATUSES.has(job.status)).length;
}

async function maybeAutoUpdateYtDlp(source) {
  const settings = await getSettings();
  if (!settings.autoUpdateYtDlp || !settings.ytDlpPath || await activeJobCount() > 0) {
    return;
  }
  const state = await chrome.storage.local.get({ lastYtDlpUpdateAt: 0, lastYtDlpUpdateAttemptAt: 0 });
  const now = Date.now();
  if (now - Number(state.lastYtDlpUpdateAt) < YT_DLP_UPDATE_INTERVAL_MS
    || now - Number(state.lastYtDlpUpdateAttemptAt) < YT_DLP_UPDATE_RETRY_MS) {
    return;
  }
  await runYtDlpUpdate(source);
}

/**
 * Runs at most one yt-dlp update at a time: install, migration, alarm, and the
 * manual button can all fire together, and two concurrent self-updates can
 * corrupt yt-dlp.exe. Downloads wait for an in-flight update before starting.
 */
let ytDlpUpdateInFlight = null;

function runYtDlpUpdate(source) {
  if (!ytDlpUpdateInFlight) {
    ytDlpUpdateInFlight = runYtDlpUpdateOnce(source).finally(() => {
      ytDlpUpdateInFlight = null;
    });
  }
  return ytDlpUpdateInFlight;
}

async function runYtDlpUpdateOnce(source) {
  if (await activeJobCount() > 0) {
    return { ok: false, busy: true, exitCode: null, output: "Downloads are running. Update again once they finish." };
  }
  const settings = await getSettings();
  await chrome.storage.local.set({ lastYtDlpUpdateAttemptAt: Date.now() });

  let response;
  try {
    response = await sendNativeMessage({ type: "updateYtDlp", settings: hostSettings(settings) });
  } catch (error) {
    response = { ok: false, exitCode: null, output: errorText(error) };
  }

  const result = {
    ok: Boolean(response?.ok),
    exitCode: response?.exitCode ?? null,
    output: String(response?.output || ""),
    source,
    at: new Date().toISOString()
  };
  // Failures surface as one popup notice driven by this result rather than
  // as entries in the downloads list, which would pile up while offline.
  await chrome.storage.local.set({
    lastYtDlpUpdateResult: result,
    ...(result.ok ? { lastYtDlpUpdateAt: Date.now() } : {})
  });
  return result;
}

async function maybeCheckExtensionUpdate(force) {
  const settings = await getSettings();
  const { extensionUpdate } = await chrome.storage.local.get({ extensionUpdate: null });
  if (!force && (!settings.checkExtensionUpdates
    || (extensionUpdate && Date.now() - Number(extensionUpdate.checkedAt || 0) < EXTENSION_UPDATE_INTERVAL_MS))) {
    return extensionUpdate;
  }

  const response = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
    headers: { Accept: "application/vnd.github+json" },
    cache: "no-store",
    credentials: "omit"
  });
  if (!response.ok) {
    throw new Error(`GitHub returned ${response.status}.`);
  }
  const release = await response.json();
  const latestVersion = String(release.tag_name || "").replace(/^v/i, "");
  const htmlUrl = String(release.html_url || "");
  const next = {
    checkedAt: Date.now(),
    currentVersion: EXTENSION_VERSION,
    latestVersion,
    // Only ever link to this repository's release pages.
    url: htmlUrl.startsWith(`${RELEASES_URL}/`) ? htmlUrl : RELEASES_URL,
    available: compareVersions(latestVersion, EXTENSION_VERSION) > 0
  };
  await chrome.storage.local.set({ extensionUpdate: next });
  return next;
}

function errorText(error) {
  return error?.message || String(error);
}
