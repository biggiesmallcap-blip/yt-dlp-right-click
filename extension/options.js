const HOST_NAME = "com.ytdlp_right_click.native_host";

const DEFAULT_SETTINGS = {
  ytDlpPath: "",
  ffmpegPath: "",
  downloadDir: "",
  cookieMode: "auto",
  displayMode: "simple",
  jsRuntime: "auto",
  jsRuntimePath: "",
  autoUpdateYtDlp: false,
  cookiesFromBrowser: false
};

const form = document.getElementById("settings-form");
const statusBox = document.getElementById("status");
const testButton = document.getElementById("test");
const updateButton = document.getElementById("update-ytdlp");
const extensionIdInput = document.getElementById("extensionId");
const installCommandInput = document.getElementById("installCommand");
const copyExtensionIdButton = document.getElementById("copy-extension-id");
const copyInstallCommandButton = document.getElementById("copy-install-command");

document.addEventListener("DOMContentLoaded", loadSettings);
form.addEventListener("submit", saveSettings);
testButton.addEventListener("click", testSettings);
updateButton.addEventListener("click", updateYtDlp);
copyExtensionIdButton.addEventListener("click", () => copyText(extensionIdInput.value, "Copied extension ID."));
copyInstallCommandButton.addEventListener("click", () => copyText(installCommandInput.value, "Copied install command."));

async function loadSettings() {
  const result = await chrome.storage.local.get({ settings: DEFAULT_SETTINGS });
  const settings = { ...DEFAULT_SETTINGS, ...result.settings };

  renderNativeSetup();
  document.getElementById("ytDlpPath").value = settings.ytDlpPath;
  document.getElementById("ffmpegPath").value = settings.ffmpegPath;
  document.getElementById("downloadDir").value = settings.downloadDir;
  document.getElementById("cookieMode").value = normalizedCookieMode(settings);
  document.getElementById("jsRuntime").value = normalizedJsRuntime(settings);
  document.getElementById("jsRuntimePath").value = settings.jsRuntimePath;
  document.getElementById("displayMode").value = normalizedDisplayMode(settings);
  document.getElementById("autoUpdateYtDlp").checked = Boolean(settings.autoUpdateYtDlp);
}

function renderNativeSetup() {
  extensionIdInput.value = chrome.runtime.id;
  installCommandInput.value = `.\\scripts\\install-native-host.ps1 -ExtensionId "${chrome.runtime.id}"`;
}

async function saveSettings(event) {
  event.preventDefault();
  const settings = readSettings();
  await chrome.storage.local.set({ settings });
  setStatus("Saved.", true);
}

async function testSettings() {
  const settings = readSettings();
  await chrome.storage.local.set({ settings });
  setStatus("Testing native host...", true);

  try {
    const response = await sendNativeMessage({
      type: "validate",
      settings
    });

    if (response?.ok) {
      const warnings = response.warnings?.length ? `\nWarnings:\n- ${response.warnings.join("\n- ")}` : "";
      setStatus(`Native host and settings look valid.${warnings}`, true);
    } else {
      const errors = response?.errors?.length ? response.errors.join("\n- ") : "Unknown validation failure.";
      setStatus(`Validation failed:\n- ${errors}`, false);
    }
  } catch (error) {
    setStatus(`Native host test failed:\n${error.message || String(error)}`, false);
  }
}

function readSettings() {
  return {
    ytDlpPath: document.getElementById("ytDlpPath").value.trim(),
    ffmpegPath: document.getElementById("ffmpegPath").value.trim(),
    downloadDir: document.getElementById("downloadDir").value.trim(),
    cookieMode: document.getElementById("cookieMode").value,
    jsRuntime: document.getElementById("jsRuntime").value,
    jsRuntimePath: document.getElementById("jsRuntimePath").value.trim(),
    displayMode: document.getElementById("displayMode").value,
    autoUpdateYtDlp: document.getElementById("autoUpdateYtDlp").checked,
    cookiesFromBrowser: false
  };
}

async function updateYtDlp() {
  const settings = readSettings();
  await chrome.storage.local.set({ settings });
  setStatus("Running yt-dlp -U...", true);

  try {
    const response = await sendRuntimeMessage({
      type: "updateYtDlp",
      settings
    });
    const output = response?.output ? `\n\n${response.output}` : "";
    if (response?.ok) {
      setStatus(`yt-dlp update completed.${output}`, true);
    } else {
      setStatus(`yt-dlp update failed with exit ${response?.exitCode ?? "unknown"}.${output}`, false);
    }
  } catch (error) {
    setStatus(`yt-dlp update failed:\n${error.message || String(error)}`, false);
  }
}

function sendRuntimeMessage(message) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendMessage(message, (response) => {
      const error = chrome.runtime.lastError;
      if (error) {
        reject(new Error(error.message));
        return;
      }
      resolve(response);
    });
  });
}

function normalizedCookieMode(settings) {
  return ["auto", "never", "always"].includes(settings.cookieMode) ? settings.cookieMode : "auto";
}

function normalizedJsRuntime(settings) {
  return ["auto", "", "deno", "node", "quickjs", "bun"].includes(settings.jsRuntime) ? settings.jsRuntime : "auto";
}

function normalizedDisplayMode(settings) {
  return settings.displayMode === "advanced" ? "advanced" : "simple";
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

async function copyText(value, message) {
  try {
    await navigator.clipboard.writeText(value);
    setStatus(message, true);
  } catch (error) {
    setStatus(`Copy failed:\n${error.message || String(error)}`, false);
  }
}

function setStatus(message, ok) {
  statusBox.textContent = message;
  statusBox.className = ok ? "ok" : "error";
}
