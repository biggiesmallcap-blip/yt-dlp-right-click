use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: u32 = 2;
const MAX_NATIVE_MESSAGE_BYTES: u32 = 1024 * 1024;
const MAX_OUTPUT_LINE_CHARS: usize = 8_000;
const MAX_COLLECTED_OUTPUT_LINES: usize = 1_000;
const MAX_COOKIES_FILE_BYTES: u64 = 16 * 1024 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_TIMEOUT: Duration = Duration::from_secs(300);
const STALE_YT_DLP_DAYS: i64 = 60;

const OUTPUT_TEMPLATE: &str = "%(title).160B.%(ext)s";
const INSTAGRAM_POST_TEMPLATE: &str =
    "%(uploader_id,channel,uploader|instagram)s/%(upload_date>%Y-%m-%d|undated)s %(id)s.%(ext)s";
const INSTAGRAM_STORY_TEMPLATE: &str =
    "%(uploader_id,channel,uploader|instagram)s/stories/%(upload_date>%Y-%m-%d|undated)s %(id)s.%(ext)s";

const LOG_DIR_NAME: &str = "_yt-dlp-right-click-logs";
const ARCHIVE_DIR_NAME: &str = "_yt-dlp-right-click-archive";
const LOG_RETENTION_DAYS: u64 = 30;
const LOG_RETENTION_MAX_FILES: usize = 300;

const FILE_MARKER: &str = "[ytdlp-rc:file]";
const TITLE_MARKER: &str = "[ytdlp-rc:title]";
const PROGRESS_MARKER: &str = "[ytdlp-rc:progress]";
const PROGRESS_TEMPLATE: &str = "download:[ytdlp-rc:progress]%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(progress.status)s|%(info.playlist_index)s|%(info.n_entries)s";

const JS_RUNTIMES: &[&str] = &["deno", "node", "quickjs", "bun"];
const COOKIE_BROWSERS: &[&str] = &[
    "chrome", "edge", "firefox", "brave", "opera", "vivaldi", "chromium", "whale",
];
const UPDATE_CHANNELS: &[&str] = &["stable", "nightly", "master"];

#[derive(Debug, Deserialize)]
struct IncomingMessage {
    #[serde(rename = "type")]
    message_type: String,
    #[serde(default, rename = "jobId")]
    job_id: String,
    #[serde(default)]
    preset: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    settings: Settings,
    #[serde(default)]
    path: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct Settings {
    #[serde(default, rename = "ytDlpPath")]
    yt_dlp_path: String,
    #[serde(default, rename = "ffmpegPath")]
    ffmpeg_path: String,
    #[serde(default, rename = "downloadDir")]
    download_dir: String,
    #[serde(default, rename = "cookieMode")]
    cookie_mode: String,
    #[serde(default, rename = "cookieBrowser")]
    cookie_browser: String,
    #[serde(default, rename = "cookiesFile")]
    cookies_file: String,
    #[serde(default, rename = "jsRuntime")]
    js_runtime: String,
    #[serde(default, rename = "jsRuntimePath")]
    js_runtime_path: String,
    #[serde(default, rename = "updateChannel")]
    update_channel: String,
    #[serde(default, rename = "embedAudioMetadata")]
    embed_audio_metadata: bool,
}

#[derive(Debug, Serialize)]
struct ValidationResponse {
    #[serde(rename = "type")]
    message_type: &'static str,
    ok: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
    #[serde(rename = "hostVersion")]
    host_version: &'static str,
    #[serde(rename = "ytDlpVersion")]
    yt_dlp_version: String,
    #[serde(rename = "ffmpegVersion")]
    ffmpeg_version: String,
}

#[derive(Debug, Serialize)]
struct VersionResponse {
    #[serde(rename = "type")]
    message_type: &'static str,
    #[serde(rename = "hostVersion")]
    host_version: &'static str,
    protocol: u32,
    presets: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    #[serde(rename = "jobId")]
    job_id: &'a str,
    message: String,
}

#[derive(Debug, Serialize)]
struct StartedResponse<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    #[serde(rename = "jobId")]
    job_id: &'a str,
    pid: u32,
    #[serde(rename = "logPath")]
    log_path: String,
    #[serde(rename = "cookiesUsed")]
    cookies_used: bool,
}

#[derive(Debug, Serialize)]
struct OutputResponse<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    #[serde(rename = "jobId")]
    job_id: &'a str,
    stream: &'static str,
    line: String,
}

#[derive(Debug, Serialize)]
struct MetaResponse<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    #[serde(rename = "jobId")]
    job_id: &'a str,
    title: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
struct Progress {
    #[serde(rename = "downloadedBytes")]
    downloaded_bytes: Option<f64>,
    #[serde(rename = "totalBytes")]
    total_bytes: Option<f64>,
    percent: Option<f64>,
    speed: Option<f64>,
    eta: Option<f64>,
    status: String,
    item: Option<u32>,
    #[serde(rename = "itemCount")]
    item_count: Option<u32>,
}

#[derive(Debug, Serialize)]
struct ProgressResponse<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    #[serde(rename = "jobId")]
    job_id: &'a str,
    #[serde(flatten)]
    progress: Progress,
}

#[derive(Debug, Serialize)]
struct FinishedResponse<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    #[serde(rename = "jobId")]
    job_id: &'a str,
    success: bool,
    cancelled: bool,
    #[serde(rename = "exitCode")]
    exit_code: i32,
    #[serde(rename = "logPath")]
    log_path: String,
    #[serde(rename = "finalPath")]
    final_path: String,
    #[serde(rename = "finalPathKind")]
    final_path_kind: &'static str,
    #[serde(rename = "fileCount")]
    file_count: usize,
}

#[derive(Debug, Serialize)]
struct UpdateResponse {
    #[serde(rename = "type")]
    message_type: &'static str,
    ok: bool,
    #[serde(rename = "exitCode")]
    exit_code: i32,
    output: String,
}

#[derive(Debug, Serialize)]
struct OpenPathResponse {
    #[serde(rename = "type")]
    message_type: &'static str,
    ok: bool,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UrlKind {
    Any,
    Instagram,
    InstagramStories,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioKind {
    None,
    Extracted,
}

#[derive(Debug, Clone)]
struct Preset {
    output_subdir: &'static str,
    output_template: &'static str,
    args: &'static [&'static str],
    playlist: bool,
    requires_ffmpeg: bool,
    url_kind: UrlKind,
    prefer_cookies: bool,
    requires_cookies: bool,
    archive_name: Option<&'static str>,
    result_kind: ResultKind,
    audio: AudioKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CookieSource {
    Browser(String),
    File(PathBuf),
}

#[derive(Debug)]
struct StartPlan {
    yt_dlp_path: PathBuf,
    target_dir: PathBuf,
    log_path: PathBuf,
    options: Vec<String>,
    url: String,
    cookie_mode: CookieMode,
    cookie_source: CookieSource,
    preset: Preset,
}

impl StartPlan {
    fn args(&self, with_cookies: bool) -> Vec<String> {
        let mut args = self.options.clone();
        if with_cookies {
            args.extend(cookie_args(&self.cookie_source));
        }
        args.push("--".to_string());
        args.push(self.url.clone());
        args
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CookieMode {
    Auto,
    Never,
    Always,
}

#[derive(Debug)]
struct RunResult {
    success: bool,
    exit_code: i32,
    output: Vec<String>,
    started_at: SystemTime,
}

#[derive(Debug, Default)]
struct CleanupReport {
    removed: Vec<PathBuf>,
    errors: Vec<String>,
}

/// Shared between the job thread and the stdin listener so a cancel request can
/// stop whichever yt-dlp attempt is currently running.
#[derive(Debug, Default)]
struct JobControl {
    cancelled: AtomicBool,
    disconnected: AtomicBool,
    pid: AtomicU32,
}

/// Serialises native messages onto stdout. Write failures are remembered but
/// never abort a running job: if Chrome goes away the download still finishes
/// and is recorded in the job log.
#[derive(Clone, Default)]
struct Outbox {
    lock: Arc<Mutex<()>>,
    broken: Arc<AtomicBool>,
}

impl Outbox {
    fn send<T: Serialize>(&self, message: &T) {
        if self.broken.load(Ordering::Relaxed) {
            return;
        }
        if write_message(message, &self.lock).is_err() {
            self.broken.store(true, Ordering::Relaxed);
        }
    }

    fn output(&self, job_id: &str, stream: &'static str, line: String) {
        self.send(&OutputResponse {
            message_type: "output",
            job_id,
            stream,
            line,
        });
    }
}

fn main() {
    let outbox = Outbox::default();

    let message = match read_message() {
        Ok(Some(message)) => message,
        Ok(None) => return,
        Err(error) => {
            outbox.send(&ErrorResponse {
                message_type: "error",
                job_id: "",
                message: error,
            });
            return;
        }
    };

    match message.message_type.as_str() {
        "version" => outbox.send(&version_response()),
        "validate" => outbox.send(&validate_settings(&message.settings, true, true)),
        "updateYtDlp" => outbox.send(&update_yt_dlp(&message.settings)),
        "openPath" => outbox.send(&open_path(&message.settings, &message.path)),
        "start" => {
            let job_id = message.job_id.clone();
            if let Err(error) = handle_start(message, &outbox) {
                let job_id = sanitize_job_id(&job_id).unwrap_or_default();
                outbox.send(&ErrorResponse {
                    message_type: "error",
                    job_id: &job_id,
                    message: error,
                });
            }
        }
        other => outbox.send(&ErrorResponse {
            message_type: "error",
            job_id: "",
            message: format!("Unsupported message type: {other}"),
        }),
    }
}

fn version_response() -> VersionResponse {
    VersionResponse {
        message_type: "version",
        host_version: HOST_VERSION,
        protocol: PROTOCOL_VERSION,
        presets: PRESET_IDS.to_vec(),
    }
}

fn handle_start(message: IncomingMessage, outbox: &Outbox) -> Result<(), String> {
    let job_id = sanitize_job_id(&message.job_id)?;
    let plan = build_start_plan(&message, &job_id)?;

    if let Some(log_dir) = plan.log_path.parent() {
        prune_old_logs(log_dir, SystemTime::now());
    }
    append_log_header(&plan.log_path, &job_id, &message, &plan)?;

    let control = Arc::new(JobControl::default());
    spawn_control_listener(control.clone());

    let cookies_first = match plan.cookie_mode {
        CookieMode::Always => true,
        CookieMode::Auto => plan.preset.prefer_cookies,
        CookieMode::Never => false,
    };

    let mut result = run_ytdlp_attempt(
        &job_id,
        &plan,
        &plan.args(cookies_first),
        cookies_first,
        outbox,
        &control,
    )?;
    let mut cookie_hint = if cookies_first {
        cookie_failure_hint(&result.output, &plan.cookie_source)
    } else {
        None
    };
    let is_cancelled = || control.cancelled.load(Ordering::SeqCst);

    if plan.cookie_mode == CookieMode::Auto
        && cookies_first
        && !plan.preset.requires_cookies
        && !result.success
        && !is_cancelled()
        && cookie_read_failed(&result.output)
    {
        // Presets that prefer cookies (Instagram posts) often work without
        // them, so an unreadable cookie store should not fail public items.
        let line = format!(
            "Could not read {}; retrying without cookies.",
            cookie_source_label(&plan.cookie_source)
        );
        outbox.output(&job_id, "hint", line.clone());
        append_host_log(&plan.log_path, &line);

        result = run_ytdlp_attempt(&job_id, &plan, &plan.args(false), false, outbox, &control)?;
    } else if plan.cookie_mode == CookieMode::Auto
        && !cookies_first
        && !result.success
        && !is_cancelled()
        && output_suggests_cookies(&result.output)
    {
        let line = format!(
            "Retrying with {} because yt-dlp reported a login/cookie requirement.",
            cookie_source_label(&plan.cookie_source)
        );
        outbox.output(&job_id, "hint", line.clone());
        append_host_log(&plan.log_path, &line);

        result = run_ytdlp_attempt(&job_id, &plan, &plan.args(true), true, outbox, &control)?;
        cookie_hint = cookie_failure_hint(&result.output, &plan.cookie_source);
    }

    // A cancel that lands after yt-dlp already exited cleanly changes nothing.
    let cancelled = is_cancelled() && !result.success;
    if cancelled {
        let line = if control.disconnected.load(Ordering::SeqCst) {
            "Chrome disconnected (browser closed or extension reloaded), so the download was stopped. Partial .part files are kept so Resume can continue."
        } else {
            "Cancelled. Partial .part files are kept so a retry can resume."
        };
        outbox.output(&job_id, "hint", line.to_string());
        append_host_log(&plan.log_path, line);
    }

    if !result.success && !cancelled {
        if let Some(hint) = cookie_hint {
            outbox.output(&job_id, "hint", hint.clone());
            append_host_log(&plan.log_path, &hint);
        }
    }

    if !result.success && !cancelled && output_suggests_old_yt_dlp(&result.output) {
        let hint = "Fix: this yt-dlp is too old for the options this extension uses. Open Settings and click Update yt-dlp now.".to_string();
        outbox.output(&job_id, "hint", hint.clone());
        append_host_log(&plan.log_path, &hint);
    }

    let files = final_output_paths_since(&result.output, result.started_at, &plan.target_dir);

    // Only trust files yt-dlp itself reported as final (after post-processing).
    // A fallback "Destination:" path can be an unconverted intermediate, e.g.
    // the .mp4 left behind when MP3 extraction fails.
    let confirmed = result
        .output
        .iter()
        .any(|line| marker_value(line, FILE_MARKER).is_some());
    if !result.success && !cancelled && confirmed && !files.is_empty() {
        let line = format!(
            "yt-dlp returned exit {}, but {} output file(s) exist. Marking complete with warnings.",
            result.exit_code,
            files.len()
        );
        outbox.output(&job_id, "hint", line.clone());
        append_host_log(&plan.log_path, &line);
        result.success = true;
    }

    if result.success && files.is_empty() && plan.preset.archive_name.is_some() {
        let line = "No new items. Everything here is already in the backup archive.".to_string();
        outbox.output(&job_id, "hint", line.clone());
        append_host_log(&plan.log_path, &line);
    }

    if result.success {
        let mut report = CleanupReport::default();
        for file in &files {
            let dir = file.parent().unwrap_or(&plan.target_dir);
            let partial = cleanup_ytdlp_temp_files(dir, Some(file), result.started_at);
            report.removed.extend(partial.removed);
            report.errors.extend(partial.errors);
        }
        report_cleanup(&job_id, &plan.log_path, report, outbox);
    }

    let (final_path, final_path_kind) = match plan.preset.result_kind {
        ResultKind::File if files.len() <= 1 => (
            files
                .first()
                .map(|path| display_path(path))
                .unwrap_or_default(),
            "file",
        ),
        _ => {
            let folder = common_parent(&files).unwrap_or_else(|| plan.target_dir.clone());
            (display_path(&folder), "folder")
        }
    };

    append_host_log(
        &plan.log_path,
        &format!(
            "finished success={} cancelled={} exit={} files={}",
            result.success,
            cancelled,
            result.exit_code,
            files.len()
        ),
    );

    outbox.send(&FinishedResponse {
        message_type: "finished",
        job_id: &job_id,
        success: result.success && !cancelled,
        cancelled,
        exit_code: result.exit_code,
        log_path: display_path(&plan.log_path),
        final_path,
        final_path_kind,
        file_count: files.len(),
    });

    Ok(())
}

/// Reads follow-up messages (currently only `cancel`) on the open port while a
/// job runs. EOF means Chrome closed the port (browser exit or extension
/// reload). Chrome terminates the host shortly after that, which would orphan
/// yt-dlp with nobody reading its output, so stop the job deliberately instead.
fn spawn_control_listener(control: Arc<JobControl>) {
    thread::spawn(move || loop {
        let stop = match read_message() {
            Ok(Some(message)) => message.message_type == "cancel",
            Ok(None) | Err(_) => {
                control.disconnected.store(true, Ordering::SeqCst);
                true
            }
        };
        if stop {
            control.cancelled.store(true, Ordering::SeqCst);
            let pid = control.pid.load(Ordering::SeqCst);
            if pid != 0 {
                kill_process_tree(pid);
            }
        }
        if control.disconnected.load(Ordering::SeqCst) {
            break;
        }
    });
}

fn kill_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        let taskkill = env::var_os("SystemRoot")
            .map(|root| PathBuf::from(root).join("System32").join("taskkill.exe"))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from("taskkill.exe"));
        let _ = Command::new(taskkill)
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }

    #[cfg(not(windows))]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn update_yt_dlp(settings: &Settings) -> UpdateResponse {
    let failed = |output: String| UpdateResponse {
        message_type: "updateResult",
        ok: false,
        exit_code: -1,
        output,
    };

    let yt_dlp_path = match resolve_yt_dlp_path(&settings.yt_dlp_path) {
        Ok(path) => path,
        Err(error) => return failed(error),
    };
    let update_args = match update_args(&settings.update_channel) {
        Ok(args) => args,
        Err(error) => return failed(error),
    };

    let mut command = Command::new(&yt_dlp_path);
    command.args(&update_args);
    if let Some(parent) = yt_dlp_path.parent() {
        command.current_dir(parent);
    }

    match capture_output(command, UPDATE_TIMEOUT) {
        Ok((code, output)) => UpdateResponse {
            message_type: "updateResult",
            ok: code == Some(0),
            exit_code: code.unwrap_or(-1),
            output: truncate_output(output),
        },
        Err(error) => failed(format!("Failed to run yt-dlp update: {error}")),
    }
}

fn update_args(channel: &str) -> Result<Vec<String>, String> {
    let channel = channel.trim().to_ascii_lowercase();
    if channel.is_empty() {
        return Ok(vec!["-U".to_string()]);
    }
    if !UPDATE_CHANNELS.contains(&channel.as_str()) {
        return Err(format!(
            "Update channel must be one of: {}.",
            UPDATE_CHANNELS.join(", ")
        ));
    }
    Ok(vec!["--update-to".to_string(), channel])
}

/// Runs a short-lived helper command, capturing stdout+stderr, and kills it if
/// it exceeds `timeout` so the extension never waits forever.
fn capture_output(
    mut command: Command,
    timeout: Duration,
) -> Result<(Option<i32>, String), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command.spawn().map_err(|error| error.to_string())?;

    let stdout = child.stdout.take().map(read_to_string_thread);
    let stderr = child.stderr.take().map(read_to_string_thread);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                kill_process_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(error.to_string()),
        }
    };

    let mut output = String::new();
    for handle in [stdout, stderr].into_iter().flatten() {
        output.push_str(&handle.join().unwrap_or_default());
    }

    match status {
        Some(status) => Ok((status.code(), output)),
        None => Ok((
            None,
            format!("{output}\nTimed out after {} seconds.", timeout.as_secs()),
        )),
    }
}

fn read_to_string_thread<R: Read + Send + 'static>(mut stream: R) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn run_ytdlp_attempt(
    job_id: &str,
    plan: &StartPlan,
    args: &[String],
    cookies_used: bool,
    outbox: &Outbox,
    control: &Arc<JobControl>,
) -> Result<RunResult, String> {
    let started_at = SystemTime::now();

    if control.cancelled.load(Ordering::SeqCst) {
        return Ok(RunResult {
            success: false,
            exit_code: -1,
            output: Vec::new(),
            started_at,
        });
    }

    let mut command = Command::new(&plan.yt_dlp_path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Output location comes from the absolute -P argument. Windows rejects a
    // working directory longer than MAX_PATH even with a \\?\ prefix, so do
    // not run inside a possibly deep download folder.
    if let Some(parent) = plan.yt_dlp_path.parent() {
        command.current_dir(parent);
    }

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    // Open the log before spawning: failing after spawn would leave yt-dlp
    // running with nobody draining its pipes.
    let log_file = Arc::new(Mutex::new(
        OpenOptions::new()
            .append(true)
            .open(&plan.log_path)
            .map_err(|error| format!("Failed to reopen log file: {error}"))?,
    ));

    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to start yt-dlp: {error}"))?;

    control.pid.store(child.id(), Ordering::SeqCst);
    // A cancel that arrived between the check above and storing the pid would
    // otherwise be missed.
    if control.cancelled.load(Ordering::SeqCst) {
        kill_process_tree(child.id());
    }

    outbox.send(&StartedResponse {
        message_type: "started",
        job_id,
        pid: child.id(),
        log_path: display_path(&plan.log_path),
        cookies_used,
    });

    let collected_output = Arc::new(Mutex::new(Vec::new()));

    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|stream| Box::new(stream) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .zip(["stdout", "stderr"])
    .filter_map(|(stream, name)| {
        stream.map(|stream| {
            spawn_reader_thread(
                job_id.to_string(),
                name,
                stream,
                outbox.clone(),
                log_file.clone(),
                collected_output.clone(),
            )
        })
    })
    .collect();

    let status = child
        .wait()
        .map_err(|error| format!("Failed while waiting for yt-dlp: {error}"))?;
    control.pid.store(0, Ordering::SeqCst);

    for handle in readers {
        let _ = handle.join();
    }

    let output = collected_output
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();

    Ok(RunResult {
        success: status.success(),
        exit_code: status.code().unwrap_or(-1),
        output,
        started_at,
    })
}

fn spawn_reader_thread(
    job_id: String,
    stream_name: &'static str,
    stream: Box<dyn Read + Send>,
    outbox: Outbox,
    log_file: Arc<Mutex<File>>,
    collected_output: Arc<Mutex<Vec<String>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut bytes = Vec::new();

        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) => break,
                Ok(_) => {
                    let line = truncate_output_line(
                        String::from_utf8_lossy(&bytes)
                            .trim_end_matches(['\r', '\n'])
                            .to_string(),
                    );
                    if line.is_empty() {
                        continue;
                    }

                    if let Some(raw) = line.strip_prefix(PROGRESS_MARKER) {
                        if let Some(progress) = parse_progress(raw) {
                            outbox.send(&ProgressResponse {
                                message_type: "progress",
                                job_id: &job_id,
                                progress,
                            });
                        }
                        continue;
                    }

                    if let Ok(mut file) = log_file.lock() {
                        let _ = writeln!(file, "[{stream_name}] {line}");
                    }
                    if let Ok(mut output) = collected_output.lock() {
                        output.push(line.clone());
                        if output.len() > MAX_COLLECTED_OUTPUT_LINES {
                            let excess = output.len() - MAX_COLLECTED_OUTPUT_LINES;
                            output.drain(0..excess);
                        }
                    }

                    if let Some(title) = marker_value(&line, TITLE_MARKER) {
                        outbox.send(&MetaResponse {
                            message_type: "meta",
                            job_id: &job_id,
                            title: title.split_whitespace().collect::<Vec<_>>().join(" "),
                        });
                        continue;
                    }

                    if let Some(path) = marker_value(&line, FILE_MARKER) {
                        outbox.output(&job_id, "file", strip_verbatim_prefix(path.trim()));
                        continue;
                    }

                    outbox.output(&job_id, stream_name, line);
                }
                Err(error) => {
                    if let Ok(mut file) = log_file.lock() {
                        let _ = writeln!(file, "[host] failed to read {stream_name}: {error}");
                    }
                    break;
                }
            }
        }
    })
}

fn parse_progress(raw: &str) -> Option<Progress> {
    let fields: Vec<&str> = raw.trim().split('|').collect();
    if fields.len() < 6 {
        return None;
    }

    let number = |value: &str| -> Option<f64> {
        value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite() && *number >= 0.0)
    };
    let integer = |value: Option<&&str>| -> Option<u32> {
        value.and_then(|value| value.trim().parse::<u32>().ok())
    };

    let downloaded_bytes = number(fields[0]);
    let total_bytes = number(fields[1]).or_else(|| number(fields[2]));
    let status = fields[5].trim();
    let percent = match (downloaded_bytes, total_bytes) {
        (Some(done), Some(total)) if total > 0.0 => Some((done / total * 100.0).min(100.0)),
        _ if status == "finished" => Some(100.0),
        _ => None,
    };

    Some(Progress {
        downloaded_bytes,
        total_bytes,
        percent,
        speed: number(fields[3]),
        eta: number(fields[4]),
        status: if status == "NA" {
            String::new()
        } else {
            status.chars().take(32).collect()
        },
        item: integer(fields.get(6)),
        item_count: integer(fields.get(7)),
    })
}

fn build_start_plan(message: &IncomingMessage, job_id: &str) -> Result<StartPlan, String> {
    let preset = get_preset(&message.preset)
        .ok_or_else(|| format!("Unsupported preset: {}", message.preset))?;
    validate_url(&message.url)?;
    let url = match preset.url_kind {
        UrlKind::Any => message.url.clone(),
        UrlKind::Instagram => {
            instagram_path_segments(&message.url)
                .ok_or("This preset only works on instagram.com links.")?;
            message.url.clone()
        }
        UrlKind::InstagramStories => instagram_stories_url(&message.url)?,
    };

    let validation = validate_settings(&message.settings, preset.requires_ffmpeg, false);
    if !validation.ok {
        return Err(validation.errors.join("; "));
    }

    let cookie_mode = cookie_mode(&message.settings);
    if preset.requires_cookies && cookie_mode == CookieMode::Never {
        return Err(
            "This preset needs your logged-in cookies. Set Cookie mode to Auto or Always in Settings."
                .to_string(),
        );
    }

    let download_root = PathBuf::from(&message.settings.download_dir);
    let target_dir = ensure_output_subdir(&download_root, preset.output_subdir)?;
    let log_dir = ensure_output_subdir(&download_root, LOG_DIR_NAME)?;
    let log_path = log_dir.join(format!("{}-{}.log", unix_seconds(), job_id));
    let yt_dlp_path = resolve_yt_dlp_path(&message.settings.yt_dlp_path)?;

    let mut options = vec![
        // yt-dlp loads <-P home>/yt-dlp.conf, and a download can create a file
        // with that name, so a malicious link could plant options (including
        // --exec) for later jobs. Every option here is fixed anyway.
        "--ignore-config".to_string(),
        "--no-quiet".to_string(),
        "--no-simulate".to_string(),
        "--windows-filenames".to_string(),
        "--trim-filenames".to_string(),
        "180".to_string(),
        "--no-mtime".to_string(),
        "--newline".to_string(),
        "--progress".to_string(),
        "--progress-delta".to_string(),
        "0.5".to_string(),
        "--progress-template".to_string(),
        PROGRESS_TEMPLATE.to_string(),
        "-P".to_string(),
        tool_path(&target_dir),
        "-o".to_string(),
        preset.output_template.to_string(),
        // `j` prints JSON strings, so multi-line titles stay on one line and
        // cannot forge marker lines.
        "--print".to_string(),
        format!("video:{TITLE_MARKER}%(title)j"),
        "--print".to_string(),
        format!("after_move:{FILE_MARKER}%(filepath)j"),
    ];

    options.push(if preset.playlist {
        "--yes-playlist".to_string()
    } else {
        "--no-playlist".to_string()
    });

    if let Some(archive_name) = preset.archive_name {
        let archive_dir = ensure_output_subdir(&download_root, ARCHIVE_DIR_NAME)?;
        options.push("--download-archive".to_string());
        options.push(tool_path(&archive_dir.join(archive_name)));
    }

    if preset.requires_ffmpeg {
        options.push("--ffmpeg-location".to_string());
        options.push(message.settings.ffmpeg_path.clone());
    }

    for js_runtime in js_runtime_args(&message.settings)? {
        options.push("--js-runtimes".to_string());
        options.push(js_runtime);
    }

    options.extend(preset.args.iter().map(|arg| (*arg).to_string()));

    if preset.audio == AudioKind::Extracted && message.settings.embed_audio_metadata {
        options.extend(
            ["--embed-metadata", "--embed-thumbnail"]
                .iter()
                .map(|arg| (*arg).to_string()),
        );
    }

    Ok(StartPlan {
        yt_dlp_path,
        target_dir,
        log_path,
        options,
        url,
        cookie_mode,
        cookie_source: cookie_source(&message.settings)?,
        preset,
    })
}

fn append_log_header(
    log_path: &Path,
    job_id: &str,
    message: &IncomingMessage,
    plan: &StartPlan,
) -> Result<(), String> {
    let mut file = File::create(log_path).map_err(|error| {
        format!(
            "Failed to create log file {}: {error}",
            display_path(log_path)
        )
    })?;

    let lines = [
        format!("yt-dlp Right Click job (host {HOST_VERSION})"),
        format!("job_id={job_id}"),
        format!("preset={}", message.preset),
        format!("url={}", plan.url),
        format!("target_dir={}", display_path(&plan.target_dir)),
        format!("yt_dlp_path={}", display_path(&plan.yt_dlp_path)),
        format!("ffmpeg_path={}", message.settings.ffmpeg_path),
        format!("cookie_mode={:?}", plan.cookie_mode),
        format!("cookie_source={}", cookie_source_label(&plan.cookie_source)),
        String::new(),
    ];
    for line in lines {
        writeln!(file, "{line}").map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn validate_settings(settings: &Settings, require_ffmpeg: bool, probe: bool) -> ValidationResponse {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut yt_dlp_version = String::new();
    let mut ffmpeg_version = String::new();

    match resolve_yt_dlp_path(&settings.yt_dlp_path) {
        Ok(path) if probe => match probe_yt_dlp_version(&path) {
            Ok(version) => {
                if let Some(age) = yt_dlp_version_age_days(&version, unix_seconds()) {
                    if age > STALE_YT_DLP_DAYS {
                        warnings.push(format!(
                            "yt-dlp {version} is {age} days old. Sites change often; run the update or enable automatic updates."
                        ));
                    }
                }
                yt_dlp_version = version;
            }
            Err(error) => errors.push(format!("yt-dlp.exe could not be run: {error}")),
        },
        Ok(_) => {}
        Err(error) => errors.push(error),
    }

    if require_ffmpeg {
        match resolve_ffmpeg_path(&settings.ffmpeg_path) {
            Ok(path) if probe => match probe_ffmpeg_version(&path) {
                Ok(version) => ffmpeg_version = version,
                Err(error) => warnings.push(format!("ffmpeg.exe could not be run: {error}")),
            },
            Ok(_) => {}
            Err(error) => errors.push(error),
        }
    }

    validate_download_root(&settings.download_dir, &mut errors);
    if let Err(error) = js_runtime_args(settings) {
        errors.push(error);
    }
    if let Err(error) = cookie_source(settings) {
        errors.push(error);
    }
    if let Err(error) = update_args(&settings.update_channel) {
        errors.push(error);
    }

    if cookie_mode(settings) != CookieMode::Never {
        if let Ok(CookieSource::Browser(browser)) = cookie_source(settings) {
            if matches!(browser.as_str(), "chrome" | "edge" | "brave" | "chromium") {
                warnings.push(format!(
                    "Cookies come from {browser}. Recent Chromium browsers on Windows encrypt cookies so yt-dlp often cannot read them. If logged-in downloads fail, export a cookies.txt file or use Firefox cookies."
                ));
            }
        }
    }

    ValidationResponse {
        message_type: "validation",
        ok: errors.is_empty(),
        errors,
        warnings,
        host_version: HOST_VERSION,
        yt_dlp_version,
        ffmpeg_version,
    }
}

fn probe_yt_dlp_version(path: &Path) -> Result<String, String> {
    let mut command = Command::new(path);
    command.arg("--version");
    let (code, output) = capture_output(command, PROBE_TIMEOUT)?;
    let version = output.lines().next().unwrap_or("").trim().to_string();
    if code == Some(0) && !version.is_empty() {
        Ok(version.chars().take(64).collect())
    } else {
        Err(format!("exit {}", code.unwrap_or(-1)))
    }
}

fn probe_ffmpeg_version(path: &Path) -> Result<String, String> {
    let mut command = Command::new(path);
    command.arg("-version");
    let (code, output) = capture_output(command, PROBE_TIMEOUT)?;
    let first = output.lines().next().unwrap_or("").trim();
    if code == Some(0) && !first.is_empty() {
        let version = first
            .strip_prefix("ffmpeg version ")
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or(first);
        Ok(version.chars().take(64).collect())
    } else {
        Err(format!("exit {}", code.unwrap_or(-1)))
    }
}

/// Age in days of a yt-dlp version string such as `2026.08.19` or a nightly
/// `2026.08.19.232803`. Returns `None` for anything that does not look like a
/// date-based version.
fn yt_dlp_version_age_days(version: &str, now_unix: u64) -> Option<i64> {
    let mut parts = version.trim().split('.');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day = parts.next()?.parse::<i64>().ok()?;
    if !(2000..=2200).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let today = (now_unix / 86_400) as i64;
    Some((today - days_from_civil(year, month, day)).max(0))
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn cookie_mode(settings: &Settings) -> CookieMode {
    match settings.cookie_mode.to_ascii_lowercase().as_str() {
        "never" => CookieMode::Never,
        "always" => CookieMode::Always,
        _ => CookieMode::Auto,
    }
}

fn cookie_source(settings: &Settings) -> Result<CookieSource, String> {
    let file = settings.cookies_file.trim();
    if !file.is_empty() {
        let path = Path::new(file);
        if !path.is_absolute() {
            return Err("cookies.txt path must be an absolute path.".to_string());
        }
        let metadata =
            fs::metadata(path).map_err(|_| "cookies.txt file does not exist.".to_string())?;
        if !metadata.is_file() {
            return Err("cookies.txt path must point to a file.".to_string());
        }
        if metadata.len() > MAX_COOKIES_FILE_BYTES {
            return Err("cookies.txt file is unexpectedly large.".to_string());
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("cookies.txt path could not be resolved: {error}"))?;
        return Ok(CookieSource::File(PathBuf::from(display_path(&canonical))));
    }

    let browser = settings.cookie_browser.trim().to_ascii_lowercase();
    if browser.is_empty() {
        return Ok(CookieSource::Browser("chrome".to_string()));
    }
    if !COOKIE_BROWSERS.contains(&browser.as_str()) {
        return Err(format!(
            "Cookie browser must be one of: {}.",
            COOKIE_BROWSERS.join(", ")
        ));
    }
    Ok(CookieSource::Browser(browser))
}

fn cookie_args(source: &CookieSource) -> Vec<String> {
    match source {
        CookieSource::Browser(browser) => {
            vec!["--cookies-from-browser".to_string(), browser.clone()]
        }
        CookieSource::File(path) => vec!["--cookies".to_string(), path.display().to_string()],
    }
}

fn cookie_source_label(source: &CookieSource) -> String {
    match source {
        CookieSource::Browser(browser) => format!("{browser} cookies"),
        CookieSource::File(_) => "cookies.txt".to_string(),
    }
}

fn js_runtime_args(settings: &Settings) -> Result<Vec<String>, String> {
    let runtime = settings.js_runtime.trim().to_ascii_lowercase();
    let runtime_path = settings.js_runtime_path.trim();

    if (runtime.is_empty() || runtime == "auto") && runtime_path.is_empty() {
        return Ok(if runtime == "auto" {
            auto_detect_js_runtimes()
        } else {
            Vec::new()
        });
    }
    if runtime.is_empty() || runtime == "auto" {
        return Err("JavaScript runtime must be selected when a runtime path is set.".to_string());
    }
    if !JS_RUNTIMES.contains(&runtime.as_str()) {
        return Err(format!(
            "JavaScript runtime must be one of: {}.",
            JS_RUNTIMES.join(", ")
        ));
    }
    if runtime_path.is_empty() {
        return Ok(vec![runtime]);
    }

    let path = Path::new(runtime_path);
    if !path.is_absolute() {
        return Err("JavaScript runtime path or folder must be an absolute path.".to_string());
    }
    if !path.exists() {
        return Err("JavaScript runtime path or folder does not exist.".to_string());
    }

    let resolved = resolve_js_runtime_path(&runtime, path)?;
    Ok(vec![format!("{runtime}:{}", display_path(&resolved))])
}

fn auto_detect_js_runtimes() -> Vec<String> {
    JS_RUNTIMES
        .iter()
        .filter_map(|runtime| {
            find_js_runtime(runtime).map(|path| format!("{runtime}:{}", display_path(&path)))
        })
        .collect()
}

fn find_js_runtime(runtime: &str) -> Option<PathBuf> {
    executable_names(runtime)
        .iter()
        .find_map(|name| find_executable_on_path(name))
        .or_else(|| {
            common_runtime_paths(runtime)
                .into_iter()
                .find(|path| path.is_file())
        })
}

fn executable_names(runtime: &str) -> Vec<String> {
    let base = match runtime {
        "deno" => "deno",
        "node" => "node",
        "quickjs" => "qjs",
        "bun" => "bun",
        _ => return Vec::new(),
    };

    if cfg!(windows) {
        vec![format!("{base}.exe"), format!("{base}.cmd")]
    } else {
        vec![base.to_string()]
    }
}

fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn common_runtime_paths(runtime: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if cfg!(windows) {
        let env_path = |key: &str| env::var_os(key).map(PathBuf::from);
        match runtime {
            "node" => {
                for key in ["ProgramFiles", "ProgramFiles(x86)"] {
                    if let Some(root) = env_path(key) {
                        paths.push(root.join("nodejs").join("node.exe"));
                    }
                }
            }
            "deno" => {
                if let Some(home) = env_path("USERPROFILE") {
                    paths.push(home.join(".deno").join("bin").join("deno.exe"));
                }
            }
            "bun" => {
                if let Some(home) = env_path("USERPROFILE") {
                    paths.push(home.join(".bun").join("bin").join("bun.exe"));
                }
            }
            _ => {}
        }
    }

    paths
}

fn resolve_js_runtime_path(runtime: &str, path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("JavaScript runtime path or folder must be an absolute path.".to_string());
    }

    if path.is_file() {
        let actual = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if !executable_names(runtime)
            .iter()
            .any(|expected| expected.eq_ignore_ascii_case(actual))
        {
            return Err(format!(
                "JavaScript runtime executable must match the selected runtime ({runtime})."
            ));
        }
        return path.canonicalize().map_err(|error| {
            format!("JavaScript runtime path or folder could not be resolved: {error}")
        });
    }

    if path.is_dir() {
        for name in executable_names(runtime) {
            let candidate = path.join(name);
            if candidate.is_file() {
                return candidate.canonicalize().map_err(|error| {
                    format!("JavaScript runtime path or folder could not be resolved: {error}")
                });
            }
        }

        return Err(format!(
            "JavaScript runtime folder does not contain the selected runtime ({runtime})."
        ));
    }

    Err("JavaScript runtime path or folder does not exist.".to_string())
}

fn output_suggests_cookies(output: &[String]) -> bool {
    let combined = output.join("\n").to_ascii_lowercase();
    [
        "sign in to confirm",
        "sign in to view",
        "login required",
        "log in to",
        "private video",
        "use --cookies",
        "cookies are required",
        "this video is private",
        "members-only",
        "join this channel",
        "age-restricted",
        "inappropriate for some users",
        "empty media response",
        "rate-limit reached or login required",
        "requested content is not available",
    ]
    .iter()
    .any(|needle| combined.contains(needle))
}

fn output_suggests_old_yt_dlp(output: &[String]) -> bool {
    output.iter().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("no such option") || lower.contains("unrecognized arguments")
    })
}

/// True when yt-dlp could not load cookies at all (as opposed to cookies
/// loading fine but the site still refusing).
fn cookie_read_failed(output: &[String]) -> bool {
    let combined = output.join("\n").to_ascii_lowercase();
    [
        "failed to decrypt with dpapi",
        "could not copy chrome cookie database",
        "app-bound",
        "could not find chrome cookies database",
        "could not find firefox cookies database",
        "cookies database",
        "permission denied",
    ]
    .iter()
    .any(|needle| combined.contains(needle))
}

fn cookie_failure_hint(output: &[String], source: &CookieSource) -> Option<String> {
    match source {
        CookieSource::Browser(browser) if cookie_read_failed(output) => Some(format!(
            "yt-dlp could not read {browser} cookies. Chromium browsers on Windows now lock/encrypt them. Fix: export a cookies.txt file (Settings > Cookies) or switch the cookie browser to Firefox."
        )),
        CookieSource::File(_) if output_suggests_cookies(output) => Some(
            "Login still required. Your cookies.txt may be expired; export a fresh one while logged in."
                .to_string(),
        ),
        _ => None,
    }
}

fn final_output_paths_since(
    output: &[String],
    since: SystemTime,
    allowed_root: &Path,
) -> Vec<PathBuf> {
    let Ok(root) = allowed_root.canonicalize() else {
        return Vec::new();
    };

    let marked: Vec<String> = output
        .iter()
        .filter_map(|line| marker_value(line, FILE_MARKER))
        .collect();

    let candidates: Vec<PathBuf> = if marked.is_empty() {
        // Older yt-dlp builds or unexpected output: fall back to the last
        // path-looking line, which is the final file for single downloads.
        output
            .iter()
            .rev()
            .filter_map(|line| output_path_from_line(line))
            .take(1)
            .collect()
    } else {
        marked
            .iter()
            .filter_map(|value| absolute_output_path_from_value(value))
            .collect()
    };

    let mut files = Vec::new();
    for path in candidates {
        if let Some(path) = canonical_file_under_root(&path, &root) {
            if file_was_written_since(&path, since) && !files.contains(&path) {
                files.push(path);
            }
        }
    }
    files
}

fn canonical_file_under_root(path: &Path, root: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }

    let canonical = path.canonicalize().ok()?;
    if canonical.starts_with(root) {
        Some(canonical)
    } else {
        None
    }
}

fn common_parent(files: &[PathBuf]) -> Option<PathBuf> {
    let mut common = files.first()?.parent()?.to_path_buf();
    for file in &files[1..] {
        while !file.starts_with(&common) {
            common = common.parent()?.to_path_buf();
        }
    }
    Some(common)
}

fn cleanup_ytdlp_temp_files(
    target_dir: &Path,
    final_path: Option<&Path>,
    since: SystemTime,
) -> CleanupReport {
    let mut report = CleanupReport::default();
    let Some(final_path) = final_path else {
        return report;
    };
    let Some(final_stem) = final_path.file_stem().and_then(|value| value.to_str()) else {
        return report;
    };

    let Ok(entries) = fs::read_dir(target_dir) else {
        return report;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path == final_path || !path.is_file() {
            continue;
        }
        if !file_was_written_since(&path, since) {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        // Require the "." after the stem: "Episode 1" must not match a
        // concurrent job's "Episode 10.f137.mp4.part".
        if !file_name.starts_with(&format!("{final_stem}.")) || !is_ytdlp_temp_artifact(file_name) {
            continue;
        }

        match fs::remove_file(&path) {
            Ok(()) => report.removed.push(path),
            Err(error) => report.errors.push(format!(
                "Failed to remove temp file {}: {error}",
                display_path(&path)
            )),
        }
    }

    report
}

fn is_ytdlp_temp_artifact(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".part")
        || lower.contains(".part-frag")
        || lower.ends_with(".ytdl")
        || lower.ends_with(".tmp")
        || lower.ends_with(".temp")
}

fn report_cleanup(job_id: &str, log_path: &Path, report: CleanupReport, outbox: &Outbox) {
    if !report.removed.is_empty() {
        let line = format!("Cleaned {} yt-dlp temp file(s).", report.removed.len());
        outbox.output(job_id, "hint", line.clone());
        append_host_log(log_path, &line);
    }

    if !report.errors.is_empty() {
        let line = report.errors.join("; ");
        outbox.output(job_id, "hint", line.clone());
        append_host_log(log_path, &line);
    }
}

/// Deletes this tool's own job logs that are older than the retention window,
/// then trims to a maximum count. Only files named like `<digits>-<job id>.log`
/// are touched.
fn prune_old_logs(log_dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(log_dir) else {
        return;
    };

    let mut logs: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(is_job_log_file_name))
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then_some(())?;
            Some((metadata.modified().ok()?, entry.path()))
        })
        .collect();

    let max_age = Duration::from_secs(LOG_RETENTION_DAYS * 86_400);
    logs.retain(|(modified, path)| {
        let expired = now
            .duration_since(*modified)
            .map(|age| age > max_age)
            .unwrap_or(false);
        if expired {
            let _ = fs::remove_file(path);
        }
        !expired
    });

    if logs.len() > LOG_RETENTION_MAX_FILES {
        logs.sort_by_key(|(modified, _)| *modified);
        let excess = logs.len() - LOG_RETENTION_MAX_FILES;
        for (_, path) in logs.into_iter().take(excess) {
            let _ = fs::remove_file(path);
        }
    }
}

fn is_job_log_file_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".log") else {
        return false;
    };
    let Some((timestamp, job_id)) = stem.split_once('-') else {
        return false;
    };
    !timestamp.is_empty()
        && timestamp.bytes().all(|byte| byte.is_ascii_digit())
        && sanitize_job_id(job_id).is_ok()
}

fn file_was_written_since(path: &Path, since: SystemTime) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    metadata
        .modified()
        .map(|modified| modified >= since)
        .unwrap_or(false)
}

/// Decodes a `[ytdlp-rc:...]` marker line. Values are printed with yt-dlp's
/// `j` (JSON) conversion so a title containing newlines cannot forge a second
/// marker line; anything that is not one well-formed JSON string is rejected.
fn marker_value(line: &str, marker: &str) -> Option<String> {
    let raw = line.strip_prefix(marker)?;
    serde_json::from_str::<String>(raw.trim()).ok()
}

fn output_path_from_line(line: &str) -> Option<PathBuf> {
    if line.starts_with(FILE_MARKER) {
        return absolute_output_path_from_value(&marker_value(line, FILE_MARKER)?);
    }

    if let Some(path) = absolute_output_path_from_value(line) {
        return Some(path);
    }

    for marker in ["Destination:", "Merging formats into", "Moving file"] {
        if let Some((_, value)) = line.rsplit_once(marker) {
            return absolute_output_path_from_value(value);
        }
    }

    None
}

fn open_path(settings: &Settings, raw_path: &str) -> OpenPathResponse {
    let result = open_folder_target(settings, raw_path)
        .and_then(|(target, select)| open_in_explorer(&target, select));

    match result {
        Ok(()) => OpenPathResponse {
            message_type: "openPathResult",
            ok: true,
            message: "Opened folder.".to_string(),
        },
        Err(message) => OpenPathResponse {
            message_type: "openPathResult",
            ok: false,
            message,
        },
    }
}

/// Resolves an open request to a path under the download root. Returns the
/// path plus whether it is a file (which Explorer should select inside its
/// folder). Missing files fall back to their nearest existing parent folder.
fn open_folder_target(settings: &Settings, raw_path: &str) -> Result<(PathBuf, bool), String> {
    let requested = PathBuf::from(raw_path.trim());
    if raw_path.trim().is_empty() || !requested.is_absolute() {
        return Err("Open path must be an absolute file or folder path.".to_string());
    }

    let download_root = PathBuf::from(settings.download_dir.trim());
    if settings.download_dir.trim().is_empty() || !download_root.is_absolute() {
        return Err("Download folder must be an absolute path.".to_string());
    }

    let Ok(root) = download_root.canonicalize() else {
        return Err("Download folder could not be resolved.".to_string());
    };

    let target = requested
        .ancestors()
        .find(|candidate| candidate.is_file() || candidate.is_dir())
        .ok_or("File or folder no longer exists.")?;

    let Ok(target_canonical) = target.canonicalize() else {
        return Err("File or folder could not be resolved.".to_string());
    };

    if !target_canonical.starts_with(&root) {
        return Err("Refusing to open a path outside the configured download folder.".to_string());
    }

    let is_file = target_canonical.is_file();
    Ok((target_canonical, is_file))
}

fn open_in_explorer(path: &Path, select: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let explorer = env::var_os("SystemRoot")
            .map(|root| PathBuf::from(root).join("explorer.exe"))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from("explorer.exe"));
        let display = display_path(path);
        let mut command = Command::new(explorer);
        if select {
            // Explorer parses `/select,"path"` itself and rejects the quoting
            // std would apply, so pass it raw. Windows paths cannot contain `"`.
            command.raw_arg(format!("/select,\"{display}\""));
        } else {
            command.arg(display);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Failed to open Explorer: {error}"))?;
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = select;
        Err(format!(
            "Opening folders is not implemented on this platform: {}",
            display_path(path)
        ))
    }
}

fn absolute_output_path_from_value(value: &str) -> Option<PathBuf> {
    let path = PathBuf::from(clean_output_path(value)?);
    if path.is_absolute() {
        Some(path)
    } else {
        None
    }
}

fn clean_output_path(value: &str) -> Option<String> {
    let trimmed = value
        .trim()
        .trim_start_matches(':')
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Removes the Windows extended-length prefix that `canonicalize` adds
/// (`\\?\C:\x` -> `C:\x`, `\\?\UNC\server\share` -> `\\server\share`) so paths
/// shown to users and passed to tools look normal.
fn strip_verbatim_prefix(value: &str) -> String {
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = value.strip_prefix(r"\\?\") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return rest.to_string();
        }
    }
    value.to_string()
}

fn display_path(path: &Path) -> String {
    strip_verbatim_prefix(&path.display().to_string())
}

/// Path form for arguments handed to yt-dlp. Short paths use the plain form so
/// output and logs stay readable; deep folders keep the `\\?\` form, which lets
/// yt-dlp and ffmpeg exceed MAX_PATH even without the LongPathsEnabled policy.
fn tool_path(path: &Path) -> String {
    const PLAIN_PATH_LIMIT: usize = 200;
    let plain = display_path(path);
    if plain.chars().count() < PLAIN_PATH_LIMIT {
        plain
    } else {
        path.display().to_string()
    }
}

fn truncate_output_line(value: String) -> String {
    if value.chars().count() <= MAX_OUTPUT_LINE_CHARS {
        return value;
    }

    let mut truncated = value
        .chars()
        .take(MAX_OUTPUT_LINE_CHARS)
        .collect::<String>();
    truncated.push_str(" [line truncated]");
    truncated
}

fn append_host_log(log_path: &Path, line: &str) {
    if let Ok(mut file) = OpenOptions::new().append(true).open(log_path) {
        let _ = writeln!(file, "[host] {line}");
    }
}

fn truncate_output(value: String) -> String {
    const MAX_CHARS: usize = 20_000;
    if value.chars().count() <= MAX_CHARS {
        return value;
    }

    let mut truncated = value.chars().take(MAX_CHARS).collect::<String>();
    truncated.push_str("\n[output truncated]");
    truncated
}

fn resolve_executable(value: &str, exe_name: &str, label: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{label} path or folder is required."));
    }

    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(format!("{label} path or folder must be an absolute path."));
    }

    let exe = if path.is_file() {
        let actual = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !actual.eq_ignore_ascii_case(exe_name) {
            return Err(format!("{label} path must point to {exe_name}."));
        }
        path.to_path_buf()
    } else if path.is_dir() {
        let exe = path.join(exe_name);
        if !exe.is_file() {
            return Err(format!("{label} folder must contain {exe_name}."));
        }
        exe
    } else {
        return Err(format!("{label} path or folder does not exist."));
    };

    exe.canonicalize()
        .map_err(|error| format!("{label} path could not be resolved: {error}"))
}

fn resolve_yt_dlp_path(value: &str) -> Result<PathBuf, String> {
    resolve_executable(value, "yt-dlp.exe", "yt-dlp.exe")
}

fn resolve_ffmpeg_path(value: &str) -> Result<PathBuf, String> {
    resolve_executable(value, "ffmpeg.exe", "ffmpeg")
}

fn validate_download_root(value: &str, errors: &mut Vec<String>) {
    let value = value.trim();
    if value.is_empty() {
        errors.push("download folder is required.".to_string());
        return;
    }

    let path = Path::new(value);
    if !path.is_absolute() {
        errors.push("download folder must be an absolute path.".to_string());
        return;
    }

    if let Err(error) = fs::create_dir_all(path) {
        errors.push(format!("download folder could not be created: {error}"));
    }
}

fn ensure_output_subdir(root: &Path, subdir: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|error| {
        format!(
            "Failed to create download root {}: {error}",
            display_path(root)
        )
    })?;
    let root_canonical = root.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve download root {}: {error}",
            display_path(root)
        )
    })?;

    let target = root_canonical.join(subdir);
    fs::create_dir_all(&target).map_err(|error| {
        format!(
            "Failed to create output folder {}: {error}",
            display_path(&target)
        )
    })?;

    let target_canonical = target.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve output folder {}: {error}",
            display_path(&target)
        )
    })?;

    if !target_canonical.starts_with(&root_canonical) {
        return Err(
            "Resolved output folder is outside the configured download folder.".to_string(),
        );
    }

    Ok(target_canonical)
}

fn validate_url(url: &str) -> Result<(), String> {
    if url.len() > 8_192 {
        return Err("URL is too long.".to_string());
    }

    let trimmed = url.trim();
    if trimmed != url {
        return Err("URL must not contain leading or trailing whitespace.".to_string());
    }

    if url
        .chars()
        .any(|value| value.is_control() || value.is_whitespace())
    {
        return Err("URL contains unsupported whitespace or control characters.".to_string());
    }

    if url.contains('\\') {
        return Err("URL contains unsupported backslash characters.".to_string());
    }

    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("Only http:// and https:// URLs are supported.".to_string());
    }

    if url_host(url).is_none() {
        return Err("URL must include a host.".to_string());
    }

    Ok(())
}

/// Lower-cased host of an http(s) URL, without userinfo or port.
fn url_host(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let scheme_len = if lower.starts_with("https://") {
        8
    } else if lower.starts_with("http://") {
        7
    } else {
        return None;
    };
    let remainder = &url[scheme_len..];
    let authority_len = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_len];
    if authority.starts_with('@') || authority.ends_with('@') {
        return None;
    }
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if host_port.starts_with('[') {
        host_port
            .split(']')
            .next()
            .map(|value| format!("{value}]"))?
    } else {
        host_port.split(':').next()?.to_string()
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// Path segments of an instagram.com URL, or `None` for any other host.
fn instagram_path_segments(url: &str) -> Option<Vec<String>> {
    let host = url_host(url)?;
    if host != "instagram.com" && !host.ends_with(".instagram.com") {
        return None;
    }
    let after_scheme = url.split_once("://")?.1;
    let path = after_scheme
        .find('/')
        .map(|index| &after_scheme[index..])
        .unwrap_or("/");
    let path = path.split(['?', '#']).next().unwrap_or("/");
    Some(
        path.split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// Maps a right-clicked Instagram URL to something yt-dlp's story extractor
/// understands. Story and highlight URLs pass through; a profile URL becomes
/// that profile's current stories.
fn instagram_stories_url(url: &str) -> Result<String, String> {
    const RESERVED: &[&str] = &[
        "p", "reel", "reels", "tv", "explore", "accounts", "direct", "about", "legal",
    ];
    let error =
        || "Open an Instagram profile, story, or highlight page to back up stories.".to_string();

    let segments = instagram_path_segments(url).ok_or_else(error)?;
    match segments.first().map(String::as_str) {
        Some("stories") if segments.len() >= 2 => Ok(url.to_string()),
        Some(user)
            if !RESERVED.contains(&user)
                && user.len() <= 30
                && user
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_') =>
        {
            Ok(format!("https://www.instagram.com/stories/{user}/"))
        }
        _ => Err(error()),
    }
}

fn sanitize_job_id(job_id: &str) -> Result<String, String> {
    if job_id.is_empty() {
        return Err("Missing job ID.".to_string());
    }

    if job_id.len() > 80 {
        return Err("Job ID is too long.".to_string());
    }

    if !job_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("Job ID contains unsupported characters.".to_string());
    }

    Ok(job_id.to_string())
}

const PRESET_IDS: &[&str] = &[
    "video_best_mp4",
    "video_1080_mp4",
    "video_720_small_mp4",
    "audio_mp3",
    "audio_mp3_320",
    "audio_m4a",
    "audio_original",
    "file_best",
    "playlist_video_best_mp4",
    "playlist_audio_mp3",
    "instagram_reel_post_mp4",
    "instagram_stories_backup",
];

const BEST_MP4: &[&str] = &[
    "-f",
    "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
    "--merge-output-format",
    "mp4",
];
const MP3_BEST: &[&str] = &["-x", "--audio-format", "mp3", "--audio-quality", "0"];

fn get_preset(id: &str) -> Option<Preset> {
    let base = Preset {
        output_subdir: "Video",
        output_template: OUTPUT_TEMPLATE,
        args: &[],
        playlist: false,
        requires_ffmpeg: true,
        url_kind: UrlKind::Any,
        prefer_cookies: false,
        requires_cookies: false,
        archive_name: None,
        result_kind: ResultKind::File,
        audio: AudioKind::None,
    };
    let audio = Preset {
        output_subdir: "Audio",
        audio: AudioKind::Extracted,
        ..base.clone()
    };
    let playlist = Preset {
        playlist: true,
        result_kind: ResultKind::Folder,
        ..base.clone()
    };
    let instagram = Preset {
        output_subdir: "Instagram",
        output_template: INSTAGRAM_POST_TEMPLATE,
        args: &["-f", "bv*+ba/b", "--merge-output-format", "mp4"],
        // Carousel posts are playlists in yt-dlp.
        playlist: true,
        url_kind: UrlKind::Instagram,
        prefer_cookies: true,
        ..base.clone()
    };

    let preset = match id {
        "video_best_mp4" => Preset {
            args: BEST_MP4,
            ..base
        },
        "video_1080_mp4" => Preset {
            args: &[
                "-f",
                "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080][ext=mp4]/best[height<=1080]",
                "--merge-output-format",
                "mp4",
            ],
            ..base
        },
        "video_720_small_mp4" => Preset {
            args: &[
                "-f",
                "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/best[height<=720][ext=mp4]/best[height<=720]",
                "--merge-output-format",
                "mp4",
            ],
            ..base
        },
        "audio_mp3" => Preset {
            args: MP3_BEST,
            ..audio
        },
        "audio_mp3_320" => Preset {
            args: &["-x", "--audio-format", "mp3", "--audio-quality", "320K"],
            ..audio
        },
        "audio_m4a" => Preset {
            args: &["-x", "--audio-format", "m4a"],
            ..audio
        },
        "audio_original" => Preset {
            args: &["-f", "bestaudio/best", "-x"],
            ..audio
        },
        "file_best" => Preset {
            output_subdir: "Files",
            args: &["-f", "b"],
            requires_ffmpeg: false,
            ..base
        },
        "playlist_video_best_mp4" => Preset {
            args: BEST_MP4,
            ..playlist
        },
        "playlist_audio_mp3" => Preset {
            output_subdir: "Audio",
            args: MP3_BEST,
            audio: AudioKind::Extracted,
            ..playlist
        },
        "instagram_reel_post_mp4" => instagram,
        "instagram_stories_backup" => Preset {
            output_template: INSTAGRAM_STORY_TEMPLATE,
            url_kind: UrlKind::InstagramStories,
            requires_cookies: true,
            archive_name: Some("instagram-stories.txt"),
            result_kind: ResultKind::Folder,
            ..instagram
        },
        _ => return None,
    };

    Some(preset)
}

fn read_message() -> Result<Option<IncomingMessage>, String> {
    let mut stdin = io::stdin().lock();
    let mut length_bytes = [0u8; 4];

    match stdin.read_exact(&mut length_bytes) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("Failed to read native message length: {error}")),
    }

    let length = u32::from_le_bytes(length_bytes);
    if length > MAX_NATIVE_MESSAGE_BYTES {
        return Err("Native message is too large.".to_string());
    }

    let mut buffer = vec![0u8; length as usize];
    stdin
        .read_exact(&mut buffer)
        .map_err(|error| format!("Failed to read native message body: {error}"))?;

    serde_json::from_slice(&buffer).map_err(|error| format!("Invalid native message JSON: {error}"))
}

fn write_message<T: Serialize>(message: &T, stdout_lock: &Arc<Mutex<()>>) -> Result<(), String> {
    let bytes = serde_json::to_vec(message)
        .map_err(|error| format!("Failed to encode response: {error}"))?;
    if bytes.len() > MAX_NATIVE_MESSAGE_BYTES as usize {
        return Err("Response message is too large.".to_string());
    }

    let _guard = stdout_lock
        .lock()
        .map_err(|_| "Failed to lock stdout.".to_string())?;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .map_err(|error| format!("Failed to write response length: {error}"))?;
    stdout
        .write_all(&bytes)
        .map_err(|error| format!("Failed to write response body: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("Failed to flush response: {error}"))
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file marker line exactly as yt-dlp prints it with `%(filepath)j`.
    fn marked(path: &Path) -> String {
        format!(
            "{FILE_MARKER}{}",
            serde_json::to_string(&path.display().to_string()).unwrap()
        )
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-host-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn plan_for(preset: &str, url: &str, settings: Settings) -> Result<StartPlan, String> {
        build_start_plan(
            &IncomingMessage {
                message_type: "start".to_string(),
                job_id: "job-1".to_string(),
                preset: preset.to_string(),
                url: url.to_string(),
                settings,
                path: String::new(),
            },
            "job-1",
        )
    }

    fn fake_tools(root: &Path) -> Settings {
        let tools = root.join("tools");
        fs::create_dir_all(&tools).unwrap();
        File::create(tools.join("yt-dlp.exe")).unwrap();
        File::create(tools.join("ffmpeg.exe")).unwrap();
        Settings {
            yt_dlp_path: tools.display().to_string(),
            ffmpeg_path: tools.display().to_string(),
            download_dir: root.join("downloads").display().to_string(),
            js_runtime: String::new(),
            ..Settings::default()
        }
    }

    #[test]
    fn rejects_non_http_urls() {
        assert!(validate_url("https://example.com/video").is_ok());
        assert!(validate_url("http://example.com/video").is_ok());
        assert!(validate_url("file:///C:/Windows/notepad.exe").is_err());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("data:text/plain,hello").is_err());
    }

    #[test]
    fn rejects_control_characters_in_urls() {
        assert!(validate_url("https://example.com/watch?v=1\n--help").is_err());
        assert!(validate_url("https://example.com/watch?v=1\t--help").is_err());
        assert!(validate_url(" https://example.com/watch").is_err());
        assert!(validate_url("https://example.com/watch ").is_err());
        assert!(validate_url("https://example.com/watch\\bad").is_err());
        assert!(validate_url("https://").is_err());
        assert!(validate_url("http:///watch").is_err());
        assert!(validate_url(&format!("https://example.com/{}", "a".repeat(9000))).is_err());
    }

    #[test]
    fn parses_url_hosts() {
        assert_eq!(
            url_host("https://user@WWW.Example.com:8080/x").as_deref(),
            Some("www.example.com")
        );
        assert_eq!(url_host("https://[::1]:80/").as_deref(), Some("[::1]"));
        assert_eq!(url_host("https://@example.com/"), None);
        assert_eq!(url_host("ftp://example.com/"), None);
    }

    #[test]
    fn every_advertised_preset_resolves() {
        for id in PRESET_IDS {
            assert!(get_preset(id).is_some(), "missing preset {id}");
        }
        assert!(get_preset("custom_args").is_none());
        assert!(get_preset("instagram_reels_backup").is_none());
    }

    #[test]
    fn sanitizes_job_ids() {
        assert!(sanitize_job_id("abc-123_DEF").is_ok());
        assert!(sanitize_job_id("../bad").is_err());
        assert!(sanitize_job_id("").is_err());
    }

    #[test]
    fn resolves_yt_dlp_folder_path() {
        let root = temp_root("resolve");
        File::create(root.join("yt-dlp.exe")).unwrap();

        let resolved = resolve_yt_dlp_path(root.to_str().unwrap()).unwrap();
        assert_eq!(resolved, root.join("yt-dlp.exe").canonicalize().unwrap());
        assert!(resolve_yt_dlp_path(root.join("yt-dlp.exe").to_str().unwrap()).is_ok());
        assert!(resolve_yt_dlp_path("relative\\yt-dlp.exe").is_err());

        File::create(root.join("other.exe")).unwrap();
        assert!(resolve_yt_dlp_path(root.join("other.exe").to_str().unwrap()).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cookie_mode_defaults_to_auto() {
        assert_eq!(cookie_mode(&Settings::default()), CookieMode::Auto);
        for (value, expected) in [
            ("always", CookieMode::Always),
            ("never", CookieMode::Never),
            ("bogus", CookieMode::Auto),
        ] {
            let settings = Settings {
                cookie_mode: value.to_string(),
                ..Settings::default()
            };
            assert_eq!(cookie_mode(&settings), expected);
        }
    }

    #[test]
    fn cookie_source_validates_browser_and_file() {
        assert_eq!(
            cookie_source(&Settings::default()).unwrap(),
            CookieSource::Browser("chrome".to_string())
        );
        let firefox = Settings {
            cookie_browser: "Firefox".to_string(),
            ..Settings::default()
        };
        assert_eq!(
            cookie_source(&firefox).unwrap(),
            CookieSource::Browser("firefox".to_string())
        );
        let bad = Settings {
            cookie_browser: "chrome --exec calc".to_string(),
            ..Settings::default()
        };
        assert!(cookie_source(&bad).is_err());

        let root = temp_root("cookies");
        let file = root.join("cookies.txt");
        File::create(&file).unwrap();
        let with_file = Settings {
            cookie_browser: "chrome".to_string(),
            cookies_file: file.display().to_string(),
            ..Settings::default()
        };
        match cookie_source(&with_file).unwrap() {
            CookieSource::File(path) => assert!(!path.display().to_string().starts_with(r"\\?\")),
            other => panic!("expected file source, got {other:?}"),
        }
        let missing = Settings {
            cookies_file: root.join("missing.txt").display().to_string(),
            ..Settings::default()
        };
        assert!(cookie_source(&missing).is_err());
        let relative = Settings {
            cookies_file: "cookies.txt".to_string(),
            ..Settings::default()
        };
        assert!(cookie_source(&relative).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plan_args_end_with_separator_and_url() {
        let root = temp_root("plan");
        let plan = plan_for(
            "audio_mp3",
            "https://example.com/video",
            Settings {
                cookie_browser: "firefox".to_string(),
                embed_audio_metadata: true,
                ..fake_tools(&root)
            },
        )
        .unwrap();

        let plain = plan.args(false);
        assert_eq!(
            &plain[plain.len() - 2..],
            ["--", "https://example.com/video"]
        );
        assert!(!plain.contains(&"--cookies-from-browser".to_string()));
        assert!(plain.contains(&"--embed-thumbnail".to_string()));
        assert!(plain.contains(&"--no-playlist".to_string()));
        assert_eq!(plain[0], "--ignore-config");
        assert!(plain.iter().any(|arg| arg.ends_with("%(title)j")));
        assert!(plain.iter().any(|arg| arg.ends_with("%(filepath)j")));

        let with_cookies = plan.args(true);
        let index = with_cookies
            .iter()
            .position(|arg| arg == "--cookies-from-browser")
            .unwrap();
        assert_eq!(with_cookies[index + 1], "firefox");
        assert!(index < with_cookies.len() - 2);

        let target = plain
            .iter()
            .position(|arg| arg == "-P")
            .map(|index| &plain[index + 1])
            .unwrap();
        assert!(!target.starts_with(r"\\?\"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn audio_metadata_is_opt_in() {
        let root = temp_root("plan-meta");
        let plan = plan_for("audio_m4a", "https://example.com/v", fake_tools(&root)).unwrap();
        assert!(!plan.args(false).contains(&"--embed-thumbnail".to_string()));
        let plan = plan_for(
            "video_best_mp4",
            "https://example.com/v",
            Settings {
                embed_audio_metadata: true,
                ..fake_tools(&root)
            },
        )
        .unwrap();
        assert!(!plan.args(false).contains(&"--embed-thumbnail".to_string()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn instagram_presets_validate_urls() {
        let root = temp_root("plan-ig");
        let settings = fake_tools(&root);

        assert!(plan_for(
            "instagram_reel_post_mp4",
            "https://www.youtube.com/watch?v=x",
            settings.clone()
        )
        .is_err());

        let post = plan_for(
            "instagram_reel_post_mp4",
            "https://www.instagram.com/reel/abc123/",
            settings.clone(),
        )
        .unwrap();
        assert!(post.args(false).contains(&"--yes-playlist".to_string()));

        let stories = plan_for(
            "instagram_stories_backup",
            "https://www.instagram.com/some.user/",
            settings.clone(),
        )
        .unwrap();
        assert_eq!(stories.url, "https://www.instagram.com/stories/some.user/");
        assert!(stories
            .args(false)
            .contains(&"--download-archive".to_string()));

        let never = Settings {
            cookie_mode: "never".to_string(),
            ..settings
        };
        assert!(plan_for(
            "instagram_stories_backup",
            "https://www.instagram.com/some.user/",
            never
        )
        .is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn maps_instagram_story_urls() {
        assert_eq!(
            instagram_stories_url("https://www.instagram.com/my_name/?hl=en").unwrap(),
            "https://www.instagram.com/stories/my_name/"
        );
        assert_eq!(
            instagram_stories_url("https://www.instagram.com/stories/highlights/1789/").unwrap(),
            "https://www.instagram.com/stories/highlights/1789/"
        );
        assert!(instagram_stories_url("https://www.instagram.com/p/abc/").is_err());
        assert!(instagram_stories_url("https://www.instagram.com/").is_err());
        assert!(instagram_stories_url("https://evil-instagram.com/user/").is_err());
        assert!(instagram_stories_url("https://instagram.com.evil.com/user/").is_err());
    }

    #[test]
    fn update_args_use_channel_allowlist() {
        assert_eq!(update_args("").unwrap(), vec!["-U"]);
        assert_eq!(
            update_args("Nightly").unwrap(),
            vec!["--update-to", "nightly"]
        );
        assert!(update_args("nightly@2020.01.01").is_err());
        assert!(update_args("--exec").is_err());
    }

    #[test]
    fn cookie_retry_detects_login_errors() {
        assert!(output_suggests_cookies(&[
            "ERROR: Sign in to confirm you are not a bot".to_string()
        ]));
        assert!(output_suggests_cookies(&[
            "ERROR: Use --cookies-from-browser or --cookies for authentication".to_string()
        ]));
        assert!(output_suggests_cookies(&[
            "ERROR: [Instagram] Da2Pt: Instagram sent an empty media response.".to_string()
        ]));
        assert!(!output_suggests_cookies(&[
            "ERROR: Requested format is not available".to_string()
        ]));
    }

    #[test]
    fn detects_options_unknown_to_old_yt_dlp() {
        assert!(output_suggests_old_yt_dlp(&[
            "yt-dlp.exe: error: no such option: --progress-delta".to_string()
        ]));
        assert!(!output_suggests_old_yt_dlp(&[
            "ERROR: Unsupported URL".to_string()
        ]));
    }

    #[test]
    fn detects_unreadable_cookie_stores() {
        assert!(cookie_read_failed(&[
            "ERROR: Failed to decrypt with DPAPI. See https://github.com/yt-dlp/yt-dlp/issues/10927".to_string()
        ]));
        assert!(!cookie_read_failed(&[
            "ERROR: Sign in to confirm you are not a bot".to_string()
        ]));
    }

    #[test]
    fn long_tool_paths_keep_extended_prefix() {
        let short = PathBuf::from(r"\\?\F:\YT-DLP\DJ\Audio");
        assert_eq!(tool_path(&short), r"F:\YT-DLP\DJ\Audio");
        let long = PathBuf::from(format!(r"\\?\F:\{}\Audio", "d".repeat(220)));
        assert!(tool_path(&long).starts_with(r"\\?\F:\"));
    }

    #[test]
    fn explains_unreadable_browser_cookies() {
        let output = vec!["ERROR: Failed to decrypt with DPAPI".to_string()];
        let hint =
            cookie_failure_hint(&output, &CookieSource::Browser("chrome".to_string())).unwrap();
        assert!(hint.contains("cookies.txt"));
        assert!(cookie_failure_hint(
            &["ERROR: Requested format is not available".to_string()],
            &CookieSource::Browser("chrome".to_string())
        )
        .is_none());
    }

    #[test]
    fn js_runtime_args_supports_runtime_without_path() {
        let settings = Settings {
            js_runtime: "node".to_string(),
            ..Settings::default()
        };
        assert_eq!(js_runtime_args(&settings).unwrap(), vec!["node"]);

        let default = Settings::default();
        assert!(js_runtime_args(&default).unwrap().is_empty());
    }

    #[test]
    fn js_runtime_args_supports_runtime_with_path_or_folder() {
        let root = temp_root("js-runtime");
        let runtime = root.join(executable_names("node").remove(0));
        File::create(&runtime).unwrap();
        let expected = vec![format!(
            "node:{}",
            display_path(&runtime.canonicalize().unwrap())
        )];

        for path in [runtime.display().to_string(), root.display().to_string()] {
            let settings = Settings {
                js_runtime: "node".to_string(),
                js_runtime_path: path,
                ..Settings::default()
            };
            assert_eq!(js_runtime_args(&settings).unwrap(), expected);
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn js_runtime_args_rejects_mismatched_runtime_executable() {
        let root = temp_root("js-mismatch");
        let runtime = root.join(executable_names("deno").remove(0));
        File::create(&runtime).unwrap();

        let settings = Settings {
            js_runtime: "node".to_string(),
            js_runtime_path: runtime.display().to_string(),
            ..Settings::default()
        };
        assert!(js_runtime_args(&settings).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn js_runtime_args_rejects_path_without_specific_runtime() {
        for runtime in ["", "auto"] {
            let settings = Settings {
                js_runtime: runtime.to_string(),
                js_runtime_path: r"C:\Tools\nodejs".to_string(),
                ..Settings::default()
            };
            assert!(js_runtime_args(&settings).is_err());
        }
    }

    #[test]
    fn output_template_uses_clean_title_only() {
        assert_eq!(OUTPUT_TEMPLATE, "%(title).160B.%(ext)s");
        assert!(!OUTPUT_TEMPLATE.contains("%(id)"));
    }

    #[test]
    fn parses_progress_lines() {
        let progress = parse_progress("1024|2048|NA|1022367.84|7|downloading|3|12").unwrap();
        assert_eq!(progress.percent, Some(50.0));
        assert_eq!(progress.eta, Some(7.0));
        assert_eq!(progress.item, Some(3));
        assert_eq!(progress.item_count, Some(12));
        assert_eq!(progress.status, "downloading");

        let estimate = parse_progress("50|NA|200.0|NA|NA|downloading|NA|NA").unwrap();
        assert_eq!(estimate.percent, Some(25.0));
        assert_eq!(estimate.speed, None);

        let finished = parse_progress("NA|NA|NA|NA|NA|finished|NA|NA").unwrap();
        assert_eq!(finished.percent, Some(100.0));

        assert!(parse_progress("garbage").is_none());
    }

    #[test]
    fn computes_yt_dlp_version_age() {
        let now = (days_from_civil(2026, 10, 15) as u64) * 86_400 + 3_600;
        assert_eq!(yt_dlp_version_age_days("2026.08.19", now), Some(57));
        assert_eq!(yt_dlp_version_age_days("2026.10.15.232803", now), Some(0));
        assert_eq!(yt_dlp_version_age_days("2027.01.01", now), Some(0));
        assert_eq!(yt_dlp_version_age_days("unknown", now), None);
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn strips_verbatim_prefixes() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\F:\YT-DLP\a.mp3"),
            r"F:\YT-DLP\a.mp3"
        );
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\a.mp3"),
            r"\\server\share\a.mp3"
        );
        assert_eq!(
            strip_verbatim_prefix(r"\\?\GLOBALROOT\x"),
            r"\\?\GLOBALROOT\x"
        );
        assert_eq!(strip_verbatim_prefix(r"C:\plain"), r"C:\plain");
    }

    #[test]
    fn detects_marked_output_files() {
        let root = temp_root("marked");
        let first = root.join("One.mp3");
        let second = root.join("Two.mp3");
        File::create(&first).unwrap();
        File::create(&second).unwrap();

        let output = vec![
            format!(
                "[download] Destination: {}",
                root.join("One.webm").display()
            ),
            marked(&first),
            marked(&second),
            marked(&second),
            // Raw (non-JSON) marker lines are ignored.
            format!("{FILE_MARKER}{}", root.join("Spoofed.mp3").display()),
        ];
        File::create(root.join("Spoofed.mp3")).unwrap();
        let files = final_output_paths_since(&output, UNIX_EPOCH, &root);
        assert_eq!(
            files,
            vec![
                first.canonicalize().unwrap(),
                second.canonicalize().unwrap()
            ]
        );
        assert!(final_output_paths_since(
            &output,
            SystemTime::now() + Duration::from_secs(60),
            &root
        )
        .is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn falls_back_to_destination_lines_without_markers() {
        let root = temp_root("fallback");
        let file = root.join("Downloaded Song.mp3");
        File::create(&file).unwrap();

        let output = vec![format!("[ExtractAudio] Destination: {}", file.display())];
        assert_eq!(
            final_output_paths_since(&output, UNIX_EPOCH, &root),
            vec![file.canonicalize().unwrap()]
        );
        assert!(final_output_paths_since(
            &output,
            SystemTime::now() + Duration::from_secs(60),
            &root
        )
        .is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_relative_printed_filepath() {
        assert_eq!(output_path_from_line("Relative Song.mp3"), None);
        assert_eq!(
            output_path_from_line("[download] Destination: Relative Song.mp3"),
            None
        );
        assert_eq!(
            output_path_from_line(&marked(Path::new("Relative Song.mp3"))),
            None
        );
    }

    #[test]
    fn marker_values_must_be_single_json_strings() {
        assert_eq!(
            marker_value(r#"[ytdlp-rc:title]"Line one\nLine two""#, TITLE_MARKER).as_deref(),
            Some("Line one\nLine two")
        );
        assert_eq!(
            marker_value("[ytdlp-rc:title]plain title", TITLE_MARKER),
            None
        );
        assert_eq!(
            marker_value(r#"[ytdlp-rc:title]"a" "b""#, TITLE_MARKER),
            None
        );
        assert_eq!(
            marker_value(r#"prefix [ytdlp-rc:file]"C:\\x""#, FILE_MARKER),
            None
        );
    }

    #[test]
    fn ignores_output_paths_outside_target_dir() {
        let root = temp_root("inside");
        let outside = temp_root("outside");
        let file = outside.join("Outside Song.mp3");
        File::create(&file).unwrap();

        let output = vec![marked(&file)];
        assert!(final_output_paths_since(&output, UNIX_EPOCH, &root).is_empty());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn finds_common_parent_folder() {
        let a = PathBuf::from(r"C:\dl\Instagram\me\stories\1.mp4");
        let b = PathBuf::from(r"C:\dl\Instagram\me\stories\2.jpg");
        let c = PathBuf::from(r"C:\dl\Instagram\me\3.mp4");
        assert_eq!(
            common_parent(&[a.clone(), b]),
            Some(PathBuf::from(r"C:\dl\Instagram\me\stories"))
        );
        assert_eq!(
            common_parent(&[a, c]),
            Some(PathBuf::from(r"C:\dl\Instagram\me"))
        );
        assert_eq!(common_parent(&[]), None);
    }

    #[test]
    fn parses_quoted_merge_output_path() {
        let path =
            output_path_from_line(r#"[Merger] Merging formats into "F:\Music\Video.mp4""#).unwrap();
        assert_eq!(path, PathBuf::from(r"F:\Music\Video.mp4"));
    }

    #[test]
    fn open_folder_target_stays_under_download_root() {
        let root = temp_root("open");
        let audio = root.join("Audio");
        fs::create_dir_all(&audio).unwrap();
        let file = audio.join("Final Song.mp3");
        File::create(&file).unwrap();
        let outside = temp_root("open-outside");

        let settings = Settings {
            download_dir: root.display().to_string(),
            ..Settings::default()
        };

        assert_eq!(
            open_folder_target(&settings, &file.display().to_string()).unwrap(),
            (file.canonicalize().unwrap(), true)
        );
        assert_eq!(
            open_folder_target(&settings, &root.display().to_string()).unwrap(),
            (root.canonicalize().unwrap(), false)
        );
        // A deleted file falls back to its folder.
        assert_eq!(
            open_folder_target(&settings, &audio.join("Gone.mp3").display().to_string()).unwrap(),
            (audio.canonicalize().unwrap(), false)
        );
        assert!(open_folder_target(&settings, &outside.display().to_string()).is_err());
        assert!(open_folder_target(&settings, "relative").is_err());

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn cleanup_removes_matching_ytdlp_temp_files_only() {
        let root = temp_root("cleanup");

        let final_file = root.join("Final Song.mp3");
        let matching_part = root.join("Final Song.mp3.part");
        let matching_fragment = root.join("Final Song.f251.webm.part-Frag12");
        let matching_ytdl = root.join("Final Song.ytdl");
        let unrelated_part = root.join("Other Song.mp4.part");
        // Another job whose title merely starts with the same text.
        let similar_title_part = root.join("Final Song 2.f137.mp4.part");
        let normal_file = root.join("Final Song Notes.txt");

        for path in [
            &final_file,
            &matching_part,
            &matching_fragment,
            &matching_ytdl,
            &unrelated_part,
            &similar_title_part,
            &normal_file,
        ] {
            File::create(path).unwrap();
        }

        let report = cleanup_ytdlp_temp_files(&root, Some(&final_file), UNIX_EPOCH);
        assert_eq!(report.removed.len(), 3);
        assert!(report.errors.is_empty());
        assert!(final_file.exists());
        assert!(!matching_part.exists());
        assert!(!matching_fragment.exists());
        assert!(!matching_ytdl.exists());
        assert!(unrelated_part.exists());
        assert!(similar_title_part.exists());
        assert!(normal_file.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prunes_only_old_job_logs() {
        let root = temp_root("logs");
        let old_log = root.join("1700000000-abc-123.log");
        let new_log = root.join("1800000000-def-456.log");
        let other = root.join("notes.log");
        for path in [&old_log, &new_log, &other] {
            File::create(path).unwrap();
        }

        let far_future = SystemTime::now() + Duration::from_secs(LOG_RETENTION_DAYS * 86_400 * 2);
        prune_old_logs(&root, far_future);
        assert!(!old_log.exists());
        assert!(!new_log.exists());
        assert!(other.exists(), "non-job files must never be pruned");

        let recent = root.join("1900000000-ghi-789.log");
        File::create(&recent).unwrap();
        prune_old_logs(&root, SystemTime::now());
        assert!(recent.exists());

        assert!(is_job_log_file_name("1792023941-2329badc-5732-46c8.log"));
        assert!(!is_job_log_file_name("x-abc.log"));
        assert!(!is_job_log_file_name("123-../evil.log"));

        fs::remove_dir_all(root).unwrap();
    }
}
