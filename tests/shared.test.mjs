import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  DEFAULT_SETTINGS,
  PRESETS,
  compareVersions,
  extractTargetUrl,
  firstUrlFromText,
  formatBytes,
  formatDuration,
  hostSettings,
  isHttpUrl,
  missingSettings,
  normalizeSettings,
  siteLabel,
  timeAgo
} from "../extension/shared.js";

test("every extension preset exists in the native host allowlist", () => {
  const source = readFileSync(new URL("../native-host/src/main.rs", import.meta.url), "utf8");
  const block = source.slice(source.indexOf("const PRESET_IDS"), source.indexOf("];", source.indexOf("const PRESET_IDS")));
  const hostIds = [...block.matchAll(/"([a-z0-9_]+)"/g)].map((match) => match[1]);
  assert.deepEqual(PRESETS.map((preset) => preset.id).sort(), [...hostIds].sort());
});

test("isHttpUrl accepts only http(s) URLs with a host", () => {
  assert.ok(isHttpUrl("https://www.youtube.com/watch?v=x"));
  assert.ok(isHttpUrl("http://example.com"));
  for (const bad of ["javascript:alert(1)", "blob:https://x.com/1", "file:///C:/x", "https://", "", null, 42]) {
    assert.equal(isHttpUrl(bad), false, String(bad));
  }
});

test("firstUrlFromText trims trailing punctuation", () => {
  assert.equal(firstUrlFromText("see (https://example.com/a)."), "https://example.com/a");
  assert.equal(firstUrlFromText("no links here"), "");
});

test("extractTargetUrl prefers links, then specific media pages over blob/CDN sources", () => {
  assert.equal(
    extractTargetUrl({ linkUrl: "https://x.com/a", pageUrl: "https://www.youtube.com/watch?v=1" }),
    "https://x.com/a"
  );
  assert.equal(
    extractTargetUrl({ srcUrl: "blob:https://www.youtube.com/abc", pageUrl: "https://www.youtube.com/watch?v=1" }),
    "https://www.youtube.com/watch?v=1"
  );
  assert.equal(
    extractTargetUrl({ srcUrl: "https://cdn.example.net/v.mp4", pageUrl: "https://www.instagram.com/p/abc/" }),
    "https://www.instagram.com/p/abc/"
  );
  // A feed/home page is not a specific item, so the media file wins.
  assert.equal(
    extractTargetUrl({ srcUrl: "https://scontent.cdninstagram.com/v.mp4", pageUrl: "https://www.instagram.com/" }),
    "https://scontent.cdninstagram.com/v.mp4"
  );
  // Right-clicked photos keep the image, not the surrounding post.
  assert.equal(
    extractTargetUrl({ mediaType: "image", srcUrl: "https://pbs.twimg.com/media/x.jpg", pageUrl: "https://x.com/u/status/1" }),
    "https://pbs.twimg.com/media/x.jpg"
  );
  assert.equal(
    extractTargetUrl({ mediaType: "video", srcUrl: "https://video.twimg.com/x.mp4", pageUrl: "https://x.com/u/status/1" }),
    "https://x.com/u/status/1"
  );
  assert.equal(
    extractTargetUrl({ frameUrl: "https://www.youtube.com/embed/abc", pageUrl: "https://blog.example.com/post" }),
    "https://www.youtube.com/embed/abc"
  );
  assert.equal(
    extractTargetUrl({ selectionText: "watch https://vimeo.com/123 now", pageUrl: "https://www.youtube.com/watch?v=1" }),
    "https://vimeo.com/123"
  );
  assert.equal(extractTargetUrl({ pageUrl: "chrome://extensions" }, { url: "chrome://extensions" }), "");
});

test("normalizeSettings fills defaults, coerces enums, and migrates v1", () => {
  const migrated = normalizeSettings({ autoUpdateYtDlp: false, cookieMode: "bogus", ytDlpPath: "  C:\\yt  " });
  assert.equal(migrated.autoUpdateYtDlp, true, "v1 settings get auto-update turned on once");
  assert.equal(migrated.cookieMode, "auto");
  assert.equal(migrated.ytDlpPath, "C:\\yt");
  assert.equal(migrated.settingsVersion, DEFAULT_SETTINGS.settingsVersion);

  const v2 = normalizeSettings({ settingsVersion: 2, autoUpdateYtDlp: false, cookieBrowser: "firefox", extra: "x" });
  assert.equal(v2.autoUpdateYtDlp, false, "an explicit v2 opt-out is kept");
  assert.equal(v2.cookieBrowser, "firefox");
  assert.equal("extra" in v2, false);

  assert.equal(normalizeSettings({ settingsVersion: 2, cookieBrowser: "netscape" }).cookieBrowser, "chrome");
  assert.equal(normalizeSettings({ settingsVersion: 2, manualPreset: "gone" }).manualPreset, DEFAULT_SETTINGS.manualPreset);
  assert.deepEqual(normalizeSettings(null), { ...DEFAULT_SETTINGS });
});

test("hostSettings only forwards host fields", () => {
  const forwarded = hostSettings(normalizeSettings({}));
  assert.equal("displayMode" in forwarded, false);
  assert.equal("notifyOnFinish" in forwarded, false);
  assert.equal("cookiesFile" in forwarded, true);
});

test("missingSettings respects ffmpeg requirements", () => {
  const settings = normalizeSettings({ ytDlpPath: "C:\\yt", downloadDir: "D:\\dl" });
  assert.deepEqual(missingSettings(settings, { requiresFfmpeg: false }), []);
  assert.deepEqual(missingSettings(settings, { requiresFfmpeg: true }), ["ffmpeg location"]);
});

test("compareVersions handles v prefixes and different lengths", () => {
  assert.equal(compareVersions("v1.10.0", "1.9.9"), 1);
  assert.equal(compareVersions("1.1", "1.1.0"), 0);
  assert.equal(compareVersions("1.0.0", "1.1.0"), -1);
});

test("formatters", () => {
  assert.equal(formatBytes(1536), "1.5 KB");
  assert.equal(formatBytes(0), "");
  assert.equal(formatDuration(75), "1m 15s");
  assert.equal(formatDuration(3700), "1h 01m");
  assert.equal(siteLabel("https://www.youtube.com/watch"), "youtube.com");
  assert.equal(siteLabel("https://music.youtube.com/watch"), "youtube.com");
  const now = Date.parse("2026-10-15T12:00:00Z");
  assert.equal(timeAgo("2026-10-15T11:59:50Z", now), "just now");
  assert.equal(timeAgo("2026-10-15T11:30:00Z", now), "30 min ago");
  assert.equal(timeAgo("2026-10-14T10:00:00Z", now), "yesterday");
});
