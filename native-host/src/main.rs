use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const MAX_NATIVE_MESSAGE_BYTES: u32 = 1024 * 1024;
const MAX_OUTPUT_LINE_CHARS: usize = 8_000;
const MAX_COLLECTED_OUTPUT_LINES: usize = 1_000;
const OUTPUT_TEMPLATE: &str = "%(title).160B.%(ext)s";

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
    #[serde(default, rename = "jsRuntime")]
    js_runtime: String,
    #[serde(default, rename = "jsRuntimePath")]
    js_runtime_path: String,
}

#[derive(Debug, Serialize)]
struct ValidationResponse {
    #[serde(rename = "type")]
    message_type: &'static str,
    ok: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
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
struct FinishedResponse<'a> {
    #[serde(rename = "type")]
    message_type: &'static str,
    #[serde(rename = "jobId")]
    job_id: &'a str,
    success: bool,
    #[serde(rename = "exitCode")]
    exit_code: i32,
    #[serde(rename = "logPath")]
    log_path: String,
    #[serde(rename = "finalPath")]
    final_path: String,
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

#[derive(Debug, Clone)]
struct Preset {
    output_subdir: &'static str,
    args: &'static [&'static str],
    playlist: bool,
    requires_ffmpeg: bool,
}

#[derive(Debug)]
struct StartPlan {
    yt_dlp_path: PathBuf,
    ffmpeg_path: PathBuf,
    target_dir: PathBuf,
    log_path: PathBuf,
    args: Vec<String>,
    cookie_mode: CookieMode,
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

fn main() {
    let stdout_lock = Arc::new(Mutex::new(()));

    match read_message() {
        Ok(Some(message)) => {
            if message.message_type == "validate" {
                let response = validate_settings(&message.settings, true);
                let _ = write_message(&response, &stdout_lock);
                return;
            }

            if message.message_type == "updateYtDlp" {
                let response = update_yt_dlp(&message.settings);
                let _ = write_message(&response, &stdout_lock);
                return;
            }

            if message.message_type == "openPath" {
                let response = open_path(&message.settings, &message.path);
                let _ = write_message(&response, &stdout_lock);
                return;
            }

            if message.message_type == "start" {
                if let Err(error) = handle_start(message, stdout_lock.clone()) {
                    let response = ErrorResponse {
                        message_type: "error",
                        job_id: "",
                        message: error,
                    };
                    let _ = write_message(&response, &stdout_lock);
                }
                return;
            }

            let response = ErrorResponse {
                message_type: "error",
                job_id: "",
                message: format!("Unsupported message type: {}", message.message_type),
            };
            let _ = write_message(&response, &stdout_lock);
        }
        Ok(None) => {}
        Err(error) => {
            let response = ErrorResponse {
                message_type: "error",
                job_id: "",
                message: error,
            };
            let _ = write_message(&response, &stdout_lock);
        }
    }
}

fn handle_start(message: IncomingMessage, stdout_lock: Arc<Mutex<()>>) -> Result<(), String> {
    let job_id = sanitize_job_id(&message.job_id)?;
    let plan = build_start_plan(&message, &job_id)?;

    append_log_header(&plan.log_path, &message, &plan)?;

    let mut first_args = plan.args.clone();
    if plan.cookie_mode == CookieMode::Always {
        first_args = args_with_chrome_cookies(&first_args);
    }

    let mut result = run_ytdlp_attempt(&job_id, &plan, &first_args, stdout_lock.clone())?;

    if plan.cookie_mode == CookieMode::Auto
        && !result.success
        && output_suggests_cookies(&result.output)
    {
        let response = OutputResponse {
            message_type: "output",
            job_id: &job_id,
            stream: "hint",
            line:
                "Retrying with Chrome cookies because yt-dlp reported a login/cookie requirement."
                    .to_string(),
        };
        write_message(&response, &stdout_lock)?;
        append_host_log(
            &plan.log_path,
            "Retrying with Chrome cookies because yt-dlp reported a login/cookie requirement.",
        );

        let retry_args = args_with_chrome_cookies(&plan.args);
        result = run_ytdlp_attempt(&job_id, &plan, &retry_args, stdout_lock.clone())?;
    }

    let final_path = final_output_path_since(&result.output, result.started_at, &plan.target_dir);

    if !result.success && final_path.is_some() {
        let line = format!(
            "yt-dlp returned exit {}, but the final output file exists. Marking complete with warnings.",
            result.exit_code
        );
        let response = OutputResponse {
            message_type: "output",
            job_id: &job_id,
            stream: "hint",
            line: line.clone(),
        };
        write_message(&response, &stdout_lock)?;
        append_host_log(&plan.log_path, &line);
        result.success = true;
    }

    if result.success {
        let cleanup =
            cleanup_ytdlp_temp_files(&plan.target_dir, final_path.as_deref(), result.started_at);
        report_cleanup(&job_id, &plan.log_path, cleanup, &stdout_lock)?;
    }

    let finished = FinishedResponse {
        message_type: "finished",
        job_id: &job_id,
        success: result.success,
        exit_code: result.exit_code,
        log_path: plan.log_path.display().to_string(),
        final_path: final_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    };
    write_message(&finished, &stdout_lock)?;

    Ok(())
}

fn update_yt_dlp(settings: &Settings) -> UpdateResponse {
    let yt_dlp_path = match resolve_yt_dlp_path(&settings.yt_dlp_path) {
        Ok(path) => path,
        Err(error) => {
            return UpdateResponse {
                message_type: "updateResult",
                ok: false,
                exit_code: -1,
                output: error,
            };
        }
    };

    let mut command = Command::new(&yt_dlp_path);
    command.arg("-U").stdin(Stdio::null());

    if let Some(parent) = yt_dlp_path.parent() {
        command.current_dir(parent);
    }

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    match command.output() {
        Ok(output) => {
            let exit_code = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = truncate_output(format!("{stdout}{stderr}"));
            UpdateResponse {
                message_type: "updateResult",
                ok: output.status.success(),
                exit_code,
                output: combined,
            }
        }
        Err(error) => UpdateResponse {
            message_type: "updateResult",
            ok: false,
            exit_code: -1,
            output: format!("Failed to run yt-dlp -U: {error}"),
        },
    }
}

fn run_ytdlp_attempt(
    job_id: &str,
    plan: &StartPlan,
    args: &[String],
    stdout_lock: Arc<Mutex<()>>,
) -> Result<RunResult, String> {
    let started_at = SystemTime::now();
    let mut command = Command::new(&plan.yt_dlp_path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(&plan.target_dir);

    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Failed to start yt-dlp: {error}"))?;

    let response = StartedResponse {
        message_type: "started",
        job_id,
        pid: child.id(),
        log_path: plan.log_path.display().to_string(),
    };
    write_message(&response, &stdout_lock)?;

    let log_file = Arc::new(Mutex::new(
        OpenOptions::new()
            .append(true)
            .open(&plan.log_path)
            .map_err(|error| format!("Failed to reopen log file: {error}"))?,
    ));
    let collected_output = Arc::new(Mutex::new(Vec::new()));

    let stdout_thread = child.stdout.take().map(|stream| {
        spawn_reader_thread(
            job_id.to_string(),
            "stdout",
            stream,
            stdout_lock.clone(),
            log_file.clone(),
            collected_output.clone(),
        )
    });

    let stderr_thread = child.stderr.take().map(|stream| {
        spawn_reader_thread(
            job_id.to_string(),
            "stderr",
            stream,
            stdout_lock.clone(),
            log_file.clone(),
            collected_output.clone(),
        )
    });

    let status = child
        .wait()
        .map_err(|error| format!("Failed while waiting for yt-dlp: {error}"))?;

    if let Some(handle) = stdout_thread {
        let _ = handle.join();
    }
    if let Some(handle) = stderr_thread {
        let _ = handle.join();
    }

    let exit_code = status.code().unwrap_or(-1);
    let output = collected_output
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();

    Ok(RunResult {
        success: status.success(),
        exit_code,
        output,
        started_at,
    })
}

fn spawn_reader_thread<R>(
    job_id: String,
    stream_name: &'static str,
    stream: R,
    stdout_lock: Arc<Mutex<()>>,
    log_file: Arc<Mutex<File>>,
    collected_output: Arc<Mutex<Vec<String>>>,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
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

                    let response = OutputResponse {
                        message_type: "output",
                        job_id: &job_id,
                        stream: stream_name,
                        line,
                    };
                    let _ = write_message(&response, &stdout_lock);
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

fn build_start_plan(message: &IncomingMessage, job_id: &str) -> Result<StartPlan, String> {
    let preset = get_preset(&message.preset)
        .ok_or_else(|| format!("Unsupported preset: {}", message.preset))?;
    validate_url(&message.url)?;

    let validation = validate_settings(&message.settings, preset.requires_ffmpeg);
    if !validation.ok {
        return Err(validation.errors.join("; "));
    }

    let download_root = PathBuf::from(&message.settings.download_dir);
    let target_dir = ensure_output_subdir(&download_root, preset.output_subdir)?;
    let log_dir = ensure_output_subdir(&download_root, "_yt-dlp-right-click-logs")?;
    let log_path = log_dir.join(format!("{}-{}.log", unix_seconds(), job_id));
    let yt_dlp_path = resolve_yt_dlp_path(&message.settings.yt_dlp_path)?;

    let mut args = vec![
        "--windows-filenames".to_string(),
        "--trim-filenames".to_string(),
        "180".to_string(),
        "--no-mtime".to_string(),
        "-P".to_string(),
        target_dir.display().to_string(),
        "-o".to_string(),
        OUTPUT_TEMPLATE.to_string(),
        "--print".to_string(),
        "after_move:filepath".to_string(),
    ];

    if preset.playlist {
        args.push("--yes-playlist".to_string());
    } else {
        args.push("--no-playlist".to_string());
    }

    if preset.requires_ffmpeg {
        args.push("--ffmpeg-location".to_string());
        args.push(message.settings.ffmpeg_path.clone());
    }

    for js_runtime in js_runtime_args(&message.settings)? {
        args.push("--js-runtimes".to_string());
        args.push(js_runtime);
    }

    args.extend(preset.args.iter().map(|arg| (*arg).to_string()));
    args.push(message.url.clone());

    Ok(StartPlan {
        yt_dlp_path,
        ffmpeg_path: PathBuf::from(&message.settings.ffmpeg_path),
        target_dir,
        log_path,
        args,
        cookie_mode: cookie_mode(&message.settings),
    })
}

fn append_log_header(
    log_path: &Path,
    message: &IncomingMessage,
    plan: &StartPlan,
) -> Result<(), String> {
    let mut file = File::create(log_path)
        .map_err(|error| format!("Failed to create log file {}: {error}", log_path.display()))?;

    writeln!(file, "yt-dlp Right Click job").map_err(|error| error.to_string())?;
    writeln!(file, "job_id={}", message.job_id).map_err(|error| error.to_string())?;
    writeln!(file, "preset={}", message.preset).map_err(|error| error.to_string())?;
    writeln!(file, "url={}", message.url).map_err(|error| error.to_string())?;
    writeln!(file, "target_dir={}", plan.target_dir.display())
        .map_err(|error| error.to_string())?;
    writeln!(file, "yt_dlp_path={}", plan.yt_dlp_path.display())
        .map_err(|error| error.to_string())?;
    writeln!(file, "ffmpeg_path={}", plan.ffmpeg_path.display())
        .map_err(|error| error.to_string())?;
    writeln!(file, "cookie_mode={:?}", cookie_mode(&message.settings))
        .map_err(|error| error.to_string())?;
    writeln!(file).map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_settings(settings: &Settings, require_ffmpeg: bool) -> ValidationResponse {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if let Err(error) = resolve_yt_dlp_path(&settings.yt_dlp_path) {
        errors.push(error);
    }

    if require_ffmpeg {
        validate_ffmpeg_path(&settings.ffmpeg_path, &mut errors);
    }

    validate_download_root(&settings.download_dir, &mut errors);
    if let Err(error) = js_runtime_args(settings) {
        errors.push(error);
    }

    if cookie_mode(settings) == CookieMode::Always {
        warnings.push(
            "Chrome cookies are always enabled. On Windows, yt-dlp may fail if Chrome is open or the cookie database is locked."
                .to_string(),
        );
    }

    ValidationResponse {
        message_type: "validation",
        ok: errors.is_empty(),
        errors,
        warnings,
    }
}

fn cookie_mode(settings: &Settings) -> CookieMode {
    match settings.cookie_mode.to_ascii_lowercase().as_str() {
        "never" => CookieMode::Never,
        "always" => CookieMode::Always,
        "auto" | "" => CookieMode::Auto,
        _ => CookieMode::Auto,
    }
}

fn js_runtime_args(settings: &Settings) -> Result<Vec<String>, String> {
    let runtime = settings.js_runtime.trim().to_ascii_lowercase();
    let runtime_path = settings.js_runtime_path.trim();

    if (runtime.is_empty() || runtime == "auto") && runtime_path.is_empty() {
        return Ok(auto_detect_js_runtimes());
    }
    if runtime.is_empty() || runtime == "auto" {
        return Err("JavaScript runtime must be selected when a runtime path is set.".to_string());
    }
    if !["deno", "node", "quickjs", "bun"].contains(&runtime.as_str()) {
        return Err("JavaScript runtime must be one of: deno, node, quickjs, bun.".to_string());
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
    Ok(vec![format!("{runtime}:{}", resolved.display())])
}

fn auto_detect_js_runtimes() -> Vec<String> {
    ["deno", "node", "quickjs", "bun"]
        .iter()
        .filter_map(|runtime| {
            find_js_runtime(runtime).map(|path| format!("{runtime}:{}", path.display()))
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
    #[cfg(windows)]
    {
        return match runtime {
            "deno" => vec!["deno.exe".to_string(), "deno.cmd".to_string()],
            "node" => vec!["node.exe".to_string(), "node.cmd".to_string()],
            "quickjs" => vec!["qjs.exe".to_string(), "qjs.cmd".to_string()],
            "bun" => vec!["bun.exe".to_string(), "bun.cmd".to_string()],
            _ => Vec::new(),
        };
    }

    #[allow(unreachable_code)]
    match runtime {
        "deno" => vec!["deno".to_string()],
        "node" => vec!["node".to_string()],
        "quickjs" => vec!["qjs".to_string()],
        "bun" => vec!["bun".to_string()],
        _ => Vec::new(),
    }
}

fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn common_runtime_paths(runtime: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(windows)]
    {
        if runtime == "node" {
            if let Some(program_files) = env::var_os("ProgramFiles") {
                paths.push(PathBuf::from(program_files).join("nodejs").join("node.exe"));
            }
            if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
                paths.push(
                    PathBuf::from(program_files_x86)
                        .join("nodejs")
                        .join("node.exe"),
                );
            }
        }
        if runtime == "deno" {
            if let Some(user_profile) = env::var_os("USERPROFILE") {
                paths.push(
                    PathBuf::from(user_profile)
                        .join(".deno")
                        .join("bin")
                        .join("deno.exe"),
                );
            }
        }
        if runtime == "bun" {
            if let Some(user_profile) = env::var_os("USERPROFILE") {
                paths.push(
                    PathBuf::from(user_profile)
                        .join(".bun")
                        .join("bin")
                        .join("bun.exe"),
                );
            }
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

fn args_with_chrome_cookies(args: &[String]) -> Vec<String> {
    let mut next = Vec::with_capacity(args.len() + 2);
    let split_at = args.len().saturating_sub(1);
    next.extend(args[..split_at].iter().cloned());
    next.push("--cookies-from-browser".to_string());
    next.push("chrome".to_string());
    next.extend(args[split_at..].iter().cloned());
    next
}

fn output_suggests_cookies(output: &[String]) -> bool {
    let combined = output.join("\n").to_ascii_lowercase();
    let cookie_needed = [
        "sign in to confirm",
        "sign in to view",
        "login required",
        "log in to",
        "private video",
        "use --cookies",
        "use --cookies-from-browser",
        "cookies are required",
        "this video is private",
        "members-only",
    ];

    cookie_needed.iter().any(|needle| combined.contains(needle))
}

fn final_output_path_since(
    output: &[String],
    since: SystemTime,
    allowed_root: &Path,
) -> Option<PathBuf> {
    let root = allowed_root.canonicalize().ok()?;

    output
        .iter()
        .filter_map(|line| output_path_from_line(line))
        .filter_map(|path| canonical_file_under_root(&path, &root))
        .find(|path| file_was_written_since(path, since))
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
        if !file_name.starts_with(final_stem) || !is_ytdlp_temp_artifact(file_name) {
            continue;
        }

        match fs::remove_file(&path) {
            Ok(()) => report.removed.push(path),
            Err(error) => report.errors.push(format!(
                "Failed to remove temp file {}: {error}",
                path.display()
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

fn report_cleanup(
    job_id: &str,
    log_path: &Path,
    report: CleanupReport,
    stdout_lock: &Arc<Mutex<()>>,
) -> Result<(), String> {
    if !report.removed.is_empty() {
        let line = format!("Cleaned {} yt-dlp temp file(s).", report.removed.len());
        let response = OutputResponse {
            message_type: "output",
            job_id,
            stream: "hint",
            line: line.clone(),
        };
        write_message(&response, stdout_lock)?;
        append_host_log(log_path, &line);
    }

    if !report.errors.is_empty() {
        let line = report.errors.join("; ");
        let response = OutputResponse {
            message_type: "output",
            job_id,
            stream: "hint",
            line: line.clone(),
        };
        write_message(&response, stdout_lock)?;
        append_host_log(log_path, &line);
    }

    Ok(())
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

fn output_path_from_line(line: &str) -> Option<PathBuf> {
    if let Some(path) = absolute_output_path_from_value(line) {
        return Some(path);
    }

    if let Some((_, value)) = line.rsplit_once("Destination:") {
        return absolute_output_path_from_value(value);
    }

    if let Some((_, value)) = line.rsplit_once("Merging formats into") {
        return absolute_output_path_from_value(value);
    }

    if let Some((_, value)) = line.rsplit_once("Moving file") {
        return absolute_output_path_from_value(value);
    }

    None
}

fn open_path(settings: &Settings, raw_path: &str) -> OpenPathResponse {
    let folder = match open_folder_target(settings, raw_path) {
        Ok(folder) => folder,
        Err(message) => {
            return OpenPathResponse {
                message_type: "openPathResult",
                ok: false,
                message,
            };
        }
    };

    match open_folder(&folder) {
        Ok(()) => OpenPathResponse {
            message_type: "openPathResult",
            ok: true,
            message: "Opened folder.".to_string(),
        },
        Err(error) => OpenPathResponse {
            message_type: "openPathResult",
            ok: false,
            message: error,
        },
    }
}

fn open_folder_target(settings: &Settings, raw_path: &str) -> Result<PathBuf, String> {
    let requested = PathBuf::from(raw_path);
    if raw_path.trim().is_empty() || !requested.is_absolute() {
        return Err("Open path must be an absolute file or folder path.".to_string());
    }

    let download_root = PathBuf::from(&settings.download_dir);
    if settings.download_dir.trim().is_empty() || !download_root.is_absolute() {
        return Err("Download folder must be an absolute path.".to_string());
    }

    let Ok(root) = download_root.canonicalize() else {
        return Err("Download folder could not be resolved.".to_string());
    };

    let target = if requested.is_file() || requested.is_dir() {
        requested.clone()
    } else {
        return Err("File or folder no longer exists.".to_string());
    };

    let Ok(target_canonical) = target.canonicalize() else {
        return Err("File or folder could not be resolved.".to_string());
    };

    if !target_canonical.starts_with(&root) {
        return Err("Refusing to open a path outside the configured download folder.".to_string());
    }

    if target_canonical.is_file() {
        Ok(target_canonical.parent().unwrap_or(&root).to_path_buf())
    } else {
        Ok(target_canonical)
    }
}

fn open_folder(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        Command::new("explorer.exe")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| format!("Failed to open Explorer: {error}"))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(format!(
        "Opening folders is not implemented on this platform: {}",
        path.display()
    ))
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

fn resolve_yt_dlp_path(value: &str) -> Result<PathBuf, String> {
    if value.trim().is_empty() {
        return Err("yt-dlp.exe path or folder is required.".to_string());
    }

    let path = Path::new(value);
    if !path.is_absolute() {
        return Err("yt-dlp.exe path or folder must be an absolute path.".to_string());
    }

    if path.is_file() {
        let actual = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if actual != "yt-dlp.exe" {
            return Err("yt-dlp executable path must point to yt-dlp.exe.".to_string());
        }
        return path
            .canonicalize()
            .map_err(|error| format!("yt-dlp executable path could not be resolved: {error}"));
    }

    if path.is_dir() {
        let exe = path.join("yt-dlp.exe");
        if exe.is_file() {
            return exe
                .canonicalize()
                .map_err(|error| format!("yt-dlp executable path could not be resolved: {error}"));
        }
        return Err("yt-dlp folder must contain yt-dlp.exe.".to_string());
    }

    Err("yt-dlp.exe path or folder does not exist.".to_string())
}

fn validate_ffmpeg_path(value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push("ffmpeg path is required for MP4/audio presets.".to_string());
        return;
    }

    let path = Path::new(value);
    if !path.is_absolute() {
        errors.push("ffmpeg path must be an absolute path.".to_string());
        return;
    }

    if path.is_file() {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name != "ffmpeg.exe" {
            errors.push("ffmpeg file path must point to ffmpeg.exe.".to_string());
        }
        return;
    }

    if path.is_dir() {
        let exe = path.join("ffmpeg.exe");
        if !exe.is_file() {
            errors.push("ffmpeg folder must contain ffmpeg.exe.".to_string());
        }
        return;
    }

    errors.push("ffmpeg path does not exist.".to_string());
}

fn validate_download_root(value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
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
    fs::create_dir_all(root)
        .map_err(|error| format!("Failed to create download root {}: {error}", root.display()))?;
    let root_canonical = root.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve download root {}: {error}",
            root.display()
        )
    })?;

    let target = root_canonical.join(subdir);
    fs::create_dir_all(&target).map_err(|error| {
        format!(
            "Failed to create output folder {}: {error}",
            target.display()
        )
    })?;

    let target_canonical = target.canonicalize().map_err(|error| {
        format!(
            "Failed to resolve output folder {}: {error}",
            target.display()
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

    let scheme_len = if lower.starts_with("https://") { 8 } else { 7 };
    let remainder = &url[scheme_len..];
    let authority_len = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_len];
    if authority.is_empty() || authority.starts_with('@') || authority.ends_with('@') {
        return Err("URL must include a host.".to_string());
    }

    Ok(())
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

fn get_preset(id: &str) -> Option<Preset> {
    let preset = match id {
        "video_best_mp4" => Preset {
            output_subdir: "Video",
            args: &[
                "-f",
                "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
                "--merge-output-format",
                "mp4",
            ],
            playlist: false,
            requires_ffmpeg: true,
        },
        "video_1080_mp4" => Preset {
            output_subdir: "Video",
            args: &[
                "-f",
                "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080][ext=mp4]/best[height<=1080]",
                "--merge-output-format",
                "mp4",
            ],
            playlist: false,
            requires_ffmpeg: true,
        },
        "video_720_small_mp4" => Preset {
            output_subdir: "Video",
            args: &[
                "-f",
                "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/best[height<=720][ext=mp4]/best[height<=720]",
                "--merge-output-format",
                "mp4",
            ],
            playlist: false,
            requires_ffmpeg: true,
        },
        "audio_mp3" => Preset {
            output_subdir: "Audio",
            args: &["-x", "--audio-format", "mp3", "--audio-quality", "0"],
            playlist: false,
            requires_ffmpeg: true,
        },
        "audio_m4a" => Preset {
            output_subdir: "Audio",
            args: &["-x", "--audio-format", "m4a"],
            playlist: false,
            requires_ffmpeg: true,
        },
        "file_best" => Preset {
            output_subdir: "Files",
            args: &["-f", "best"],
            playlist: false,
            requires_ffmpeg: false,
        },
        "playlist_video_best_mp4" => Preset {
            output_subdir: "Video",
            args: &[
                "-f",
                "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
                "--merge-output-format",
                "mp4",
            ],
            playlist: true,
            requires_ffmpeg: true,
        },
        "playlist_audio_mp3" => Preset {
            output_subdir: "Audio",
            args: &["-x", "--audio-format", "mp3", "--audio-quality", "0"],
            playlist: true,
            requires_ffmpeg: true,
        },
        _ => return None,
    };

    Some(preset)
}

fn read_message() -> Result<Option<IncomingMessage>, String> {
    let mut stdin = io::stdin();
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
    let mut stdout = io::stdout();
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
    }

    #[test]
    fn rejects_unknown_presets() {
        assert!(get_preset("audio_mp3").is_some());
        assert!(get_preset("custom_args").is_none());
    }

    #[test]
    fn sanitizes_job_ids() {
        assert!(sanitize_job_id("abc-123_DEF").is_ok());
        assert!(sanitize_job_id("../bad").is_err());
        assert!(sanitize_job_id("").is_err());
    }

    #[test]
    fn resolves_yt_dlp_folder_path() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-host-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();
        File::create(root.join("yt-dlp.exe")).unwrap();

        let resolved = resolve_yt_dlp_path(root.to_str().unwrap()).unwrap();
        assert_eq!(resolved, root.join("yt-dlp.exe").canonicalize().unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cookie_mode_defaults_to_auto() {
        let settings = Settings::default();
        assert_eq!(cookie_mode(&settings), CookieMode::Auto);

        let settings = Settings {
            cookie_mode: "always".to_string(),
            ..Settings::default()
        };
        assert_eq!(cookie_mode(&settings), CookieMode::Always);

        let settings = Settings {
            cookie_mode: "never".to_string(),
            ..Settings::default()
        };
        assert_eq!(cookie_mode(&settings), CookieMode::Never);
    }

    #[test]
    fn cookie_args_are_inserted_before_url() {
        let args = vec![
            "--no-playlist".to_string(),
            "-x".to_string(),
            "https://example.com/video".to_string(),
        ];
        let with_cookies = args_with_chrome_cookies(&args);
        assert_eq!(
            with_cookies,
            vec![
                "--no-playlist",
                "-x",
                "--cookies-from-browser",
                "chrome",
                "https://example.com/video",
            ]
        );
    }

    #[test]
    fn cookie_retry_detects_login_errors() {
        assert!(output_suggests_cookies(&[
            "ERROR: Sign in to confirm you are not a bot".to_string()
        ]));
        assert!(output_suggests_cookies(&[
            "ERROR: Use --cookies-from-browser or --cookies for authentication".to_string()
        ]));
        assert!(!output_suggests_cookies(&[
            "ERROR: Requested format is not available".to_string()
        ]));
    }

    #[test]
    fn js_runtime_args_supports_runtime_without_path() {
        let settings = Settings {
            js_runtime: "node".to_string(),
            ..Settings::default()
        };

        assert_eq!(
            js_runtime_args(&settings).unwrap(),
            vec!["node".to_string()]
        );
    }

    #[test]
    fn js_runtime_args_supports_runtime_with_path() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-js-runtime-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();
        let runtime_name = executable_names("node").remove(0);
        let runtime = root.join(runtime_name);
        File::create(&runtime).unwrap();

        let settings = Settings {
            js_runtime: "node".to_string(),
            js_runtime_path: runtime.display().to_string(),
            ..Settings::default()
        };

        assert_eq!(
            js_runtime_args(&settings).unwrap(),
            vec![format!(
                "node:{}",
                runtime.canonicalize().unwrap().display()
            )]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn js_runtime_args_supports_runtime_folder_path() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-js-runtime-folder-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();
        let runtime = root.join(executable_names("node").remove(0));
        File::create(&runtime).unwrap();

        let settings = Settings {
            js_runtime: "node".to_string(),
            js_runtime_path: root.display().to_string(),
            ..Settings::default()
        };

        assert_eq!(
            js_runtime_args(&settings).unwrap(),
            vec![format!(
                "node:{}",
                runtime.canonicalize().unwrap().display()
            )]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn js_runtime_args_rejects_mismatched_runtime_executable() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-js-runtime-mismatch-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();
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
    fn js_runtime_args_rejects_path_without_runtime() {
        let settings = Settings {
            js_runtime_path: r"C:\Tools\nodejs".to_string(),
            ..Settings::default()
        };

        assert!(js_runtime_args(&settings).is_err());
    }

    #[test]
    fn js_runtime_args_rejects_path_with_auto_runtime() {
        let settings = Settings {
            js_runtime: "auto".to_string(),
            js_runtime_path: r"C:\Tools\nodejs".to_string(),
            ..Settings::default()
        };

        assert!(js_runtime_args(&settings).is_err());
    }

    #[test]
    fn executable_names_include_windows_variants() {
        let names = executable_names("node");
        assert!(names
            .iter()
            .any(|name| name == "node.exe" || name == "node"));
    }

    #[test]
    fn output_template_uses_clean_title_only() {
        assert_eq!(OUTPUT_TEMPLATE, "%(title).160B.%(ext)s");
        assert!(!OUTPUT_TEMPLATE.contains("%(id)"));
    }

    #[test]
    fn detects_existing_destination_file() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-output-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("Downloaded Song.mp3");
        File::create(&file).unwrap();

        let output = vec![format!("[ExtractAudio] Destination: {}", file.display())];
        assert_eq!(
            final_output_path_since(&output, UNIX_EPOCH, &root),
            Some(file.canonicalize().unwrap())
        );
        assert_eq!(
            final_output_path_since(
                &output,
                SystemTime::now() + std::time::Duration::from_secs(60),
                &root
            ),
            None
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detects_printed_after_move_filepath() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-after-move-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("Final Song.mp3");
        File::create(&file).unwrap();

        let output = vec![file.display().to_string()];
        assert_eq!(
            final_output_path_since(&output, UNIX_EPOCH, &root),
            Some(file.canonicalize().unwrap())
        );
        assert_eq!(
            final_output_path_since(
                &output,
                SystemTime::now() + std::time::Duration::from_secs(60),
                &root
            ),
            None
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ignores_stale_destination_file() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-stale-output-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("Old Song.mp3");
        File::create(&file).unwrap();

        let output = vec![format!("[ExtractAudio] Destination: {}", file.display())];
        assert_eq!(
            final_output_path_since(
                &output,
                SystemTime::now() + std::time::Duration::from_secs(60),
                &root
            ),
            None
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_relative_printed_filepath() {
        assert_eq!(output_path_from_line("Relative Song.mp3"), None);
        assert_eq!(
            output_path_from_line("[download] Destination: Relative Song.mp3"),
            None
        );

        let root = std::env::temp_dir().join(format!(
            "ytdlp-relative-output-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();
        assert_eq!(
            final_output_path_since(&["Relative Song.mp3".to_string()], UNIX_EPOCH, &root),
            None
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ignores_output_paths_outside_target_dir() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-inside-output-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        let outside = std::env::temp_dir().join(format!(
            "ytdlp-outside-output-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let file = outside.join("Outside Song.mp3");
        File::create(&file).unwrap();

        let output = vec![format!("[ExtractAudio] Destination: {}", file.display())];
        assert_eq!(final_output_path_since(&output, UNIX_EPOCH, &root), None);

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn parses_quoted_merge_output_path() {
        let path =
            output_path_from_line(r#"[Merger] Merging formats into "F:\Music\Video.mp4""#).unwrap();
        assert_eq!(path, PathBuf::from(r"F:\Music\Video.mp4"));
    }

    #[test]
    fn open_folder_target_uses_parent_for_files() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-open-folder-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        let audio = root.join("Audio");
        fs::create_dir_all(&audio).unwrap();
        let file = audio.join("Final Song.mp3");
        File::create(&file).unwrap();

        let settings = Settings {
            download_dir: root.display().to_string(),
            ..Settings::default()
        };

        assert_eq!(
            open_folder_target(&settings, &file.display().to_string()).unwrap(),
            audio.canonicalize().unwrap()
        );
        assert_eq!(
            open_folder_target(&settings, &root.display().to_string()).unwrap(),
            root.canonicalize().unwrap()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_removes_matching_ytdlp_temp_files_only() {
        let root = std::env::temp_dir().join(format!(
            "ytdlp-cleanup-test-{}-{}",
            std::process::id(),
            unix_seconds()
        ));
        fs::create_dir_all(&root).unwrap();

        let final_file = root.join("Final Song.mp3");
        let matching_part = root.join("Final Song.mp3.part");
        let matching_fragment = root.join("Final Song.f251.webm.part-Frag12");
        let matching_ytdl = root.join("Final Song.ytdl");
        let unrelated_part = root.join("Other Song.mp4.part");
        let normal_file = root.join("Final Song Notes.txt");

        for path in [
            &final_file,
            &matching_part,
            &matching_fragment,
            &matching_ytdl,
            &unrelated_part,
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
        assert!(normal_file.exists());

        fs::remove_dir_all(root).unwrap();
    }
}
