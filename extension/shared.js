// Shared constants and pure helpers for the service worker, popup, and
// options page. Keep this file free of chrome.* calls so it can be unit
// tested with plain Node.

export const HOST_NAME = "com.ytdlp_right_click.native_host";
export const REPO = "biggiesmallcap-blip/yt-dlp-right-click";
export const RELEASES_URL = `https://github.com/${REPO}/releases`;
export const SETTINGS_VERSION = 2;
export const MAX_JOBS = 40;
export const MAX_OUTPUT_LINES = 150;

// Preset IDs must match the native host allowlist (native-host/src/main.rs).
export const PRESETS = [
  { id: "video_best_mp4", title: "Best MP4", group: "video", requiresFfmpeg: true },
  { id: "video_1080_mp4", title: "MP4 up to 1080p", group: "video", requiresFfmpeg: true },
  { id: "video_720_small_mp4", title: "Small MP4 up to 720p", group: "video", requiresFfmpeg: true },
  { id: "audio_mp3", title: "MP3 (VBR best)", group: "audio", requiresFfmpeg: true },
  { id: "audio_mp3_320", title: "MP3 320 kbps", group: "audio", requiresFfmpeg: true },
  { id: "audio_m4a", title: "M4A", group: "audio", requiresFfmpeg: true },
  { id: "audio_original", title: "Original audio, no re-encode", group: "audio", requiresFfmpeg: true },
  { id: "file_best", title: "Best single file", group: "file", requiresFfmpeg: false },
  { id: "playlist_video_best_mp4", title: "Whole playlist as MP4", group: "playlist", requiresFfmpeg: true },
  { id: "playlist_audio_mp3", title: "Whole playlist as MP3", group: "playlist", requiresFfmpeg: true },
  { id: "instagram_reel_post_mp4", title: "Post, reel, or carousel", group: "instagram", requiresFfmpeg: true },
  { id: "instagram_stories_backup", title: "Stories and highlights backup", group: "instagram", requiresFfmpeg: true }
];

export const GROUP_TITLES = {
  video: "Video",
  audio: "Audio",
  file: "File",
  playlist: "Playlist",
  instagram: "Instagram"
};

export const COOKIE_BROWSERS = ["chrome", "edge", "firefox", "brave", "opera", "vivaldi", "chromium", "whale"];
export const UPDATE_CHANNELS = ["", "stable", "nightly", "master"];
export const JS_RUNTIMES = ["auto", "", "deno", "node", "quickjs", "bun"];

export const DEFAULT_SETTINGS = {
  settingsVersion: SETTINGS_VERSION,
  ytDlpPath: "",
  ffmpegPath: "",
  downloadDir: "",
  cookieMode: "auto",
  cookieBrowser: "chrome",
  cookiesFile: "",
  displayMode: "simple",
  manualPreset: "video_best_mp4",
  jsRuntime: "auto",
  jsRuntimePath: "",
  autoUpdateYtDlp: true,
  updateChannel: "",
  embedAudioMetadata: true,
  notifyOnFinish: true,
  checkExtensionUpdates: true
};

export const ACTIVE_STATUSES = new Set(["starting", "running"]);

export function presetById(id) {
  return PRESETS.find((preset) => preset.id === id);
}

/** Fill defaults, drop unknown keys, and coerce enum fields to valid values. */
export function normalizeSettings(raw) {
  const input = raw && typeof raw === "object" ? raw : {};
  const settings = { ...DEFAULT_SETTINGS };
  for (const key of Object.keys(DEFAULT_SETTINGS)) {
    if (key in input && typeof input[key] === typeof DEFAULT_SETTINGS[key]) {
      settings[key] = input[key];
    }
  }

  // v1 shipped with automatic yt-dlp updates off and no way to tell an
  // explicit "off" from the default; v2 turns them on once.
  if (Number(input.settingsVersion || 1) < 2) {
    settings.autoUpdateYtDlp = true;
  }
  settings.settingsVersion = SETTINGS_VERSION;

  for (const key of ["ytDlpPath", "ffmpegPath", "downloadDir", "cookiesFile", "jsRuntimePath"]) {
    settings[key] = settings[key].trim();
  }
  if (!["auto", "never", "always"].includes(settings.cookieMode)) settings.cookieMode = "auto";
  if (!COOKIE_BROWSERS.includes(settings.cookieBrowser)) settings.cookieBrowser = "chrome";
  if (!UPDATE_CHANNELS.includes(settings.updateChannel)) settings.updateChannel = "";
  if (!JS_RUNTIMES.includes(settings.jsRuntime)) settings.jsRuntime = "auto";
  if (settings.displayMode !== "advanced") settings.displayMode = "simple";
  if (!presetById(settings.manualPreset)) settings.manualPreset = DEFAULT_SETTINGS.manualPreset;
  return settings;
}

/** Only the fields the native host understands; UI preferences stay local. */
export function hostSettings(settings) {
  const {
    ytDlpPath, ffmpegPath, downloadDir, cookieMode, cookieBrowser, cookiesFile,
    jsRuntime, jsRuntimePath, updateChannel, embedAudioMetadata
  } = settings;
  return {
    ytDlpPath, ffmpegPath, downloadDir, cookieMode, cookieBrowser, cookiesFile,
    jsRuntime, jsRuntimePath, updateChannel, embedAudioMetadata
  };
}

export function missingSettings(settings, preset) {
  const missing = [];
  if (!settings.ytDlpPath) missing.push("yt-dlp location");
  if (preset?.requiresFfmpeg !== false && !settings.ffmpegPath) missing.push("ffmpeg location");
  if (!settings.downloadDir) missing.push("download folder");
  if (settings.jsRuntimePath && (!settings.jsRuntime || settings.jsRuntime === "auto")) {
    missing.push("a specific JavaScript runtime for the custom runtime path");
  }
  return missing;
}

export function isHttpUrl(value) {
  if (typeof value !== "string" || !/^https?:\/\//i.test(value)) {
    return false;
  }
  try {
    const url = new URL(value);
    return (url.protocol === "http:" || url.protocol === "https:") && Boolean(url.hostname);
  } catch {
    return false;
  }
}

export function firstUrlFromText(text) {
  if (!text) return "";
  const match = String(text).match(/https?:\/\/[^\s"'<>]+/i);
  return match ? match[0].replace(/[),.;!?]+$/, "") : "";
}

/** Pick the most specific http(s) URL from a context-menu click. */
export function extractTargetUrl(info, tab) {
  const pageUrl = info?.pageUrl || tab?.url || "";
  // On known media sites the page URL carries the metadata yt-dlp wants, while
  // a media srcUrl is usually a blob: or a bare CDN chunk. Feed/home pages
  // ("/") are not a specific item, so those still fall through to srcUrl.
  const specificMediaPage = (url) => isHttpUrl(url) && isKnownMediaPage(url) && new URL(url).pathname !== "/";
  const candidates = [
    info?.linkUrl,
    firstUrlFromText(info?.selectionText),
    specificMediaPage(info?.frameUrl) ? info.frameUrl : undefined,
    specificMediaPage(pageUrl) ? pageUrl : undefined,
    info?.srcUrl,
    info?.frameUrl,
    pageUrl
  ];
  return candidates.find((candidate) => isHttpUrl(candidate)) || "";
}

const MEDIA_PAGE_HOSTS = [
  "youtube.com",
  "youtu.be",
  "soundcloud.com",
  "vimeo.com",
  "twitch.tv",
  "tiktok.com",
  "instagram.com",
  "x.com",
  "twitter.com",
  "facebook.com",
  "reddit.com",
  "bandcamp.com",
  "mixcloud.com",
  "dailymotion.com"
];

export function isKnownMediaPage(url) {
  const host = hostOf(url);
  return MEDIA_PAGE_HOSTS.some((domain) => host === domain || host.endsWith(`.${domain}`));
}

export function hostOf(url) {
  try {
    return new URL(url).hostname.toLowerCase();
  } catch {
    return "";
  }
}

export function siteLabel(url) {
  return hostOf(url).replace(/^(www|m|music|mobile)\./, "");
}

/** Compare dotted numeric versions ("1.10.0" > "1.9.2"); ignores a leading v. */
export function compareVersions(a, b) {
  const parse = (value) => String(value || "")
    .trim()
    .replace(/^v/i, "")
    .split(/[.+-]/)
    .map((part) => Number.parseInt(part, 10))
    .map((part) => (Number.isFinite(part) ? part : 0));
  const left = parse(a);
  const right = parse(b);
  for (let index = 0; index < Math.max(left.length, right.length, 3); index += 1) {
    const diff = (left[index] || 0) - (right[index] || 0);
    if (diff !== 0) return diff > 0 ? 1 : -1;
  }
  return 0;
}

export function formatBytes(bytes) {
  const value = Number(bytes);
  if (!Number.isFinite(value) || value <= 0) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size >= 100 || unit === 0 ? Math.round(size) : size.toFixed(1)} ${units[unit]}`;
}

export function formatDuration(seconds) {
  const value = Math.round(Number(seconds));
  if (!Number.isFinite(value) || value < 0) return "";
  if (value < 60) return `${value}s`;
  const minutes = Math.floor(value / 60);
  if (minutes < 60) return `${minutes}m ${String(value % 60).padStart(2, "0")}s`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}

export function timeAgo(iso, now = Date.now()) {
  const then = Date.parse(iso);
  if (!Number.isFinite(then)) return "";
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 45) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

export function basename(path) {
  return String(path || "")
    .replace(/^\\\\\?\\/, "")
    .split(/[\\/]/)
    .filter(Boolean)
    .pop() || String(path || "");
}

export function stripExtension(name) {
  return String(name).replace(/\.[A-Za-z0-9]{1,5}$/, "");
}
