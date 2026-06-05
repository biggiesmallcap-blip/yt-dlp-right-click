const HOST_NAME = "com.ytdlp_right_click.native_host";
const AUTO_UPDATE_SUCCESS_INTERVAL_MS = 24 * 60 * 60 * 1000;
const AUTO_UPDATE_FAILURE_COOLDOWN_MS = 6 * 60 * 60 * 1000;

const MENU_CONTEXTS = ["page", "link", "selection", "image", "video", "audio"];

const PRESETS = [
  { id: "video_best_mp4", title: "Best MP4", group: "video", requiresFfmpeg: true },
  { id: "video_1080_mp4", title: "MP4 up to 1080p", group: "video", requiresFfmpeg: true },
  { id: "video_720_small_mp4", title: "Small MP4 up to 720p", group: "video", requiresFfmpeg: true },
  { id: "audio_mp3", title: "MP3", group: "audio", requiresFfmpeg: true },
  { id: "audio_m4a", title: "M4A", group: "audio", requiresFfmpeg: true },
  { id: "file_best", title: "Best available file", group: "file", requiresFfmpeg: false },
  { id: "playlist_video_best_mp4", title: "Playlist best MP4", group: "playlist", requiresFfmpeg: true },
  { id: "playlist_audio_mp3", title: "Playlist MP3 audio", group: "playlist", requiresFfmpeg: true }
];

const DEFAULT_SETTINGS = {
  ytDlpPath: "",
  ffmpegPath: "",
  downloadDir: "",
  cookieMode: "auto",
  displayMode: "simple",
  manualPreset: "video_best_mp4",
  jsRuntime: "auto",
  jsRuntimePath: "",
  autoUpdateYtDlp: false,
  cookiesFromBrowser: false
};

const GROUP_TITLES = {
  video: "Video",
  audio: "Audio",
  file: "File",
  playlist: "Playlist"
};

chrome.runtime.onInstalled.addListener(async () => {
  await ensureDefaultSettings();
  createContextMenus();
  await updateBadge();
  maybeAutoUpdateYtDlp("installed").catch(() => {});
});

chrome.runtime.onStartup.addListener(async () => {
  await markInterruptedJobs();
  await updateBadge();
  maybeAutoUpdateYtDlp("startup").catch(() => {});
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  handleContextMenuClick(info, tab).catch((error) => {
    recordLocalError("Menu action failed", error.message || String(error));
  });
});

chrome.storage.onChanged.addListener((changes, areaName) => {
  if (areaName === "local" && changes.settings?.newValue?.autoUpdateYtDlp) {
    maybeAutoUpdateYtDlp("settings").catch(() => {});
  }
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "clearCompletedJobs") {
    clearCompletedJobs().then(() => sendResponse({ ok: true }));
    return true;
  }

  if (message?.type === "updateYtDlp") {
    updateYtDlpManually(message.settings || {})
      .then((response) => sendResponse(response))
      .catch((error) => sendResponse({
        ok: false,
        exitCode: null,
        output: error.message || String(error)
      }));
    return true;
  }

  if (message?.type === "retryJob") {
    retryJob(message.jobId)
      .then((response) => sendResponse(response))
      .catch((error) => sendResponse({ ok: false, message: error.message || String(error) }));
    return true;
  }

  if (message?.type === "openJobFolder") {
    openJobFolder(message.jobId)
      .then((response) => sendResponse(response))
      .catch((error) => sendResponse({ ok: false, message: error.message || String(error) }));
    return true;
  }

  if (message?.type === "startDownload") {
    startDownload(message.presetId, message.url)
      .then((response) => sendResponse(response))
      .catch((error) => sendResponse({ ok: false, message: error.message || String(error) }));
    return true;
  }

  if (message?.type === "getPresets") {
    sendResponse({ ok: true, presets: PRESETS });
    return false;
  }

  return false;
});

async function ensureDefaultSettings() {
  const current = await chrome.storage.local.get({ settings: DEFAULT_SETTINGS });
  await chrome.storage.local.set({
    settings: { ...DEFAULT_SETTINGS, ...current.settings }
  });
}

function createContextMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: "root",
      title: "yt-dlp",
      contexts: MENU_CONTEXTS
    });

    for (const group of Object.keys(GROUP_TITLES)) {
      chrome.contextMenus.create({
        id: `group_${group}`,
        parentId: "root",
        title: GROUP_TITLES[group],
        contexts: MENU_CONTEXTS
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
  });
}

async function handleContextMenuClick(info, tab) {
  const preset = PRESETS.find((item) => item.id === info.menuItemId);
  if (!preset) {
    return;
  }

  const url = extractTargetUrl(info, tab);
  if (!url) {
    await recordLocalError("No supported URL", "Right-click a page, link, media item, or selected http(s) URL.");
    return;
  }

  const response = await startDownload(preset.id, url);
  if (!response.ok) {
    await recordLocalError("Download failed", response.message || "Could not start download.");
  }
}

function extractTargetUrl(info, tab) {
  const candidates = [
    info.linkUrl,
    isHttpUrl(info.srcUrl) ? info.srcUrl : undefined,
    firstUrlFromText(info.selectionText),
    info.pageUrl,
    tab?.url
  ];

  return candidates.find((candidate) => isHttpUrl(candidate)) || "";
}

function firstUrlFromText(text) {
  if (!text) {
    return "";
  }

  const match = text.match(/https?:\/\/[^\s"'<>]+/i);
  return match ? match[0] : "";
}

function isHttpUrl(value) {
  return typeof value === "string" && /^https?:\/\//i.test(value);
}

async function maybeAutoUpdateYtDlp(source) {
  const settings = await getSettings();
  if (!settings.autoUpdateYtDlp || !settings.ytDlpPath) {
    return;
  }
  if (await activeJobCount() > 0) {
    return;
  }

  const state = await chrome.storage.local.get({
    lastYtDlpUpdateAt: 0,
    lastYtDlpUpdateAttemptAt: 0
  });
  const now = Date.now();
  if (now - Number(state.lastYtDlpUpdateAt || 0) < AUTO_UPDATE_SUCCESS_INTERVAL_MS) {
    return;
  }
  if (now - Number(state.lastYtDlpUpdateAttemptAt || 0) < AUTO_UPDATE_FAILURE_COOLDOWN_MS) {
    return;
  }

  await chrome.storage.local.set({ lastYtDlpUpdateAttemptAt: now });

  try {
    const response = await sendNativeMessage({
      type: "updateYtDlp",
      settings
    });
    await chrome.storage.local.set({
      lastYtDlpUpdateResult: {
        ok: Boolean(response?.ok),
        exitCode: response?.exitCode ?? null,
        output: response?.output || "",
        source,
        at: new Date().toISOString()
      }
    });

    if (response?.ok) {
      await chrome.storage.local.set({ lastYtDlpUpdateAt: now });
    }

    if (!response?.ok) {
      await recordLocalError("yt-dlp update failed", compactUpdateOutput(response));
    }
  } catch (error) {
    await chrome.storage.local.set({
      lastYtDlpUpdateResult: {
        ok: false,
        exitCode: null,
        output: error.message || String(error),
        source,
        at: new Date().toISOString()
      }
    });
    await recordLocalError("yt-dlp update failed", error.message || String(error));
  }
}

async function updateYtDlpManually(settings) {
  if (await activeJobCount() > 0) {
    return {
      ok: false,
      exitCode: null,
      output: "Wait for active downloads to finish before updating yt-dlp."
    };
  }

  try {
    const response = await sendNativeMessage({
      type: "updateYtDlp",
      settings
    });
    await chrome.storage.local.set({
      lastYtDlpUpdateResult: {
        ok: Boolean(response?.ok),
        exitCode: response?.exitCode ?? null,
        output: response?.output || "",
        source: "manual",
        at: new Date().toISOString()
      }
    });
    if (response?.ok) {
      await chrome.storage.local.set({ lastYtDlpUpdateAt: Date.now() });
    }
    return response;
  } catch (error) {
    return {
      ok: false,
      exitCode: null,
      output: error.message || String(error)
    };
  }
}

function sendNativeMessage(message) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendNativeMessage(HOST_NAME, message, (response) => {
      const error = chrome.runtime.lastError;
      if (error) {
        reject(new Error(error.message));
        return;
      }
      resolve(response);
    });
  });
}

function compactUpdateOutput(response) {
  const output = response?.output || "Unknown update failure.";
  return `exit ${response?.exitCode ?? "unknown"}\n${output}`.trim();
}

async function getSettings() {
  const result = await chrome.storage.local.get({ settings: DEFAULT_SETTINGS });
  return { ...DEFAULT_SETTINGS, ...result.settings };
}

function missingSettings(settings, preset) {
  const missing = [];
  if (!settings.ytDlpPath) {
    missing.push("yt-dlp.exe path or folder");
  }
  if (preset?.requiresFfmpeg && !settings.ffmpegPath) {
    missing.push("ffmpeg path");
  }
  if (!settings.downloadDir) {
    missing.push("download folder");
  }
  if (settings.jsRuntimePath && (!settings.jsRuntime || settings.jsRuntime === "auto")) {
    missing.push("specific JavaScript runtime name");
  }
  return missing;
}

async function startNativeJob({ preset, presetTitle, url, settings }) {
  const jobId = crypto.randomUUID();
  const job = {
    id: jobId,
    preset,
    presetTitle,
    url,
    status: "starting",
    output: [],
    logPath: "",
    finalPath: "",
    startedAt: new Date().toISOString(),
    finishedAt: ""
  };

  await upsertJob(job);
  await updateBadge();

  let port;
  try {
    port = chrome.runtime.connectNative(HOST_NAME);
  } catch (error) {
    await finishJobWithError(jobId, `Native host unavailable: ${error.message || String(error)}`);
    return;
  }

  port.onMessage.addListener((message) => {
    handleNativeMessage(jobId, message).catch((error) => {
      recordLocalError("Native message handling failed", error.message || String(error));
    });
  });

  port.onDisconnect.addListener(() => {
    const message = chrome.runtime.lastError?.message;
    if (message) {
      handleNativeDisconnect(jobId, message).catch(() => {});
    }
  });

  port.postMessage({
    type: "start",
    jobId,
    preset,
    url,
    settings
  });
}

function nativeHostErrorMessage(message) {
  const lower = String(message).toLowerCase();
  if (lower.includes("specified native messaging host not found")) {
    return [
      "Native host is not registered for this Chrome extension ID.",
      "Fix: open chrome://extensions, copy this extension ID, then run:",
      ".\\scripts\\install-native-host.ps1 -ExtensionId \"<extension-id>\""
    ].join("\n");
  }

  if (lower.includes("access to the specified native messaging host is forbidden")) {
    return [
      "Native host is registered, but it does not allow this Chrome extension ID.",
      "Fix: rerun the installer with the exact extension ID shown in chrome://extensions:",
      ".\\scripts\\install-native-host.ps1 -ExtensionId \"<extension-id>\""
    ].join("\n");
  }

  if (lower.includes("native host has exited")) {
    return "Native host started but exited early. Open Settings and run the native host test.";
  }

  return `Native host disconnected: ${message}`;
}

async function handleNativeDisconnect(jobId, message) {
  const jobs = await getJobs();
  const job = jobs.find((item) => item.id === jobId);
  if (job && (job.status === "complete" || job.status === "failed")) {
    return;
  }

  await finishJobWithError(jobId, nativeHostErrorMessage(message));
}

async function handleNativeMessage(jobId, message) {
  if (!message || typeof message.type !== "string") {
    return;
  }

  if (message.type === "started") {
    await patchJob(jobId, {
      status: "running",
      pid: message.pid || 0,
      logPath: message.logPath || ""
    });
    return;
  }

  if (message.type === "output") {
    await appendJobOutput(jobId, message.line || "", message.stream || "stdout");
    if (isChromeCookieCopyError(message.line || "")) {
      await appendCookieHint(jobId);
    }
    return;
  }

  if (message.type === "finished") {
    await patchJob(jobId, {
      status: message.success ? "complete" : "failed",
      exitCode: message.exitCode,
      finishedAt: new Date().toISOString(),
      logPath: message.logPath || "",
      finalPath: message.finalPath || ""
    });
    await updateBadge();
    return;
  }

  if (message.type === "error") {
    await finishJobWithError(jobId, message.message || "Unknown native host error");
  }
}

function isChromeCookieCopyError(line) {
  return String(line).toLowerCase().includes("could not copy chrome cookie database");
}

async function appendCookieHint(jobId) {
  const hint = [
    "Cookie fix: disable Chrome cookies in Settings for public videos.",
    "If cookies are required, fully close Chrome first, or use a cookies.txt workflow instead."
  ].join(" ");
  const jobs = await getJobs();
  const job = jobs.find((item) => item.id === jobId);
  const output = job?.output || [];
  if (!output.some((entry) => entry.line === hint)) {
    await appendJobOutput(jobId, hint, "hint");
  }
}

async function recordLocalError(title, detail) {
  const jobId = crypto.randomUUID();
  await upsertJob({
    id: jobId,
    preset: "local_error",
    presetTitle: title,
    url: "",
    status: "failed",
    output: [{ stream: "error", line: detail, at: new Date().toISOString() }],
    logPath: "",
    finalPath: "",
    startedAt: new Date().toISOString(),
    finishedAt: new Date().toISOString()
  });
  await updateBadge();
}

async function retryJob(jobId) {
  const jobs = await getJobs();
  const job = jobs.find((item) => item.id === jobId);
  if (!job) {
    return { ok: false, message: "Job not found." };
  }
  if (!job.url || !job.preset) {
    return { ok: false, message: "Job cannot be retried because it has no saved URL or preset." };
  }
  if (job.status === "running" || job.status === "starting") {
    return { ok: false, message: "Job is already running." };
  }

  const preset = PRESETS.find((item) => item.id === job.preset);
  if (!preset) {
    return { ok: false, message: "Saved preset is no longer supported." };
  }

  const settings = await getSettings();
  const missing = missingSettings(settings, preset);
  if (missing.length > 0) {
    return { ok: false, message: `Missing: ${missing.join(", ")}.` };
  }

  await rememberManualPreset(preset.id, settings);

  await startNativeJob({
    preset: preset.id,
    presetTitle: preset.title,
    url: job.url,
    settings
  });

  return { ok: true };
}

async function startDownload(presetId, url) {
  const preset = PRESETS.find((item) => item.id === presetId);
  if (!preset) {
    return { ok: false, message: "Unsupported preset." };
  }

  if (!isHttpUrl(url)) {
    return { ok: false, message: "No supported http(s) URL is available for this tab." };
  }

  const settings = await getSettings();
  const missing = missingSettings(settings, preset);
  if (missing.length > 0) {
    return { ok: false, message: `Missing: ${missing.join(", ")}. Open Settings.` };
  }

  await rememberManualPreset(preset.id, settings);

  await startNativeJob({
    preset: preset.id,
    presetTitle: preset.title,
    url,
    settings
  });

  return { ok: true };
}

async function rememberManualPreset(presetId, settings) {
  await chrome.storage.local.set({
    settings: {
      ...settings,
      manualPreset: presetId
    }
  });
}

async function openJobFolder(jobId) {
  const jobs = await getJobs();
  const job = jobs.find((item) => item.id === jobId);
  if (!job) {
    return { ok: false, message: "Job not found." };
  }

  const settings = await getSettings();
  const path = job.finalPath || settings.downloadDir;
  if (!path) {
    return { ok: false, message: "No download folder is configured." };
  }

  try {
    const response = await sendNativeMessage({
      type: "openPath",
      path,
      settings
    });
    return {
      ok: Boolean(response?.ok),
      message: response?.message || ""
    };
  } catch (error) {
    return { ok: false, message: error.message || String(error) };
  }
}

async function finishJobWithError(jobId, message) {
  await appendJobOutput(jobId, message, "error");
  await patchJob(jobId, {
    status: "failed",
    finishedAt: new Date().toISOString()
  });
  await updateBadge();
}

async function appendJobOutput(jobId, line, stream) {
  const jobs = await getJobs();
  const job = jobs.find((item) => item.id === jobId);
  if (!job) {
    return;
  }

  const output = [
    ...(job.output || []),
    { stream, line, at: new Date().toISOString() }
  ].slice(-120);

  await upsertJob({ ...job, output });
}

async function patchJob(jobId, patch) {
  const jobs = await getJobs();
  const next = jobs.map((job) => (job.id === jobId ? { ...job, ...patch } : job));
  await chrome.storage.local.set({ jobs: next.slice(0, 30) });
}

async function upsertJob(job) {
  const jobs = await getJobs();
  const without = jobs.filter((item) => item.id !== job.id);
  await chrome.storage.local.set({ jobs: [job, ...without].slice(0, 30) });
}

async function getJobs() {
  const result = await chrome.storage.local.get({ jobs: [] });
  return Array.isArray(result.jobs) ? result.jobs : [];
}

async function activeJobCount() {
  const jobs = await getJobs();
  return jobs.filter((job) => job.status === "running" || job.status === "starting").length;
}

async function clearCompletedJobs() {
  const jobs = await getJobs();
  const active = jobs.filter((job) => job.status === "running" || job.status === "starting");
  await chrome.storage.local.set({ jobs: active });
  await updateBadge();
}

async function markInterruptedJobs() {
  const jobs = await getJobs();
  const now = new Date().toISOString();
  const next = jobs.map((job) => {
    if (job.status === "running" || job.status === "starting") {
      return {
        ...job,
        status: "failed",
        finishedAt: now,
        output: [
          ...(job.output || []),
          { stream: "error", line: "Browser restarted before this job reported completion.", at: now }
        ].slice(-120)
      };
    }
    return job;
  });
  await chrome.storage.local.set({ jobs: next });
}

async function updateBadge() {
  const jobs = await getJobs();
  const running = jobs.filter((job) => job.status === "running" || job.status === "starting").length;
  await chrome.action.setBadgeText({ text: running ? String(running) : "" });
  await chrome.action.setBadgeBackgroundColor({ color: running ? "#1d4ed8" : "#64748b" });
}
