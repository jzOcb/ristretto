//! Lifecycle hook loading and execution.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use git2::Repository;
use rist_shared::{HookConfig, HookEvent, HookResult, SessionId};
use serde::Deserialize;
use serde::Serialize;

const HOOKS_FILE: &str = ".ristretto/hooks.toml";
const AUDIT_FILE: &str = ".ristretto/hook-audit.jsonl";
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Default)]
pub struct HookEngine {
    last_run_at: Mutex<HashMap<DebounceKey, Instant>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookRunOutcome {
    pub results: Vec<HookResult>,
    pub blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DebounceKey {
    session_id: SessionId,
    event: HookEvent,
    hook_index: usize,
}

#[derive(Debug, Deserialize)]
struct HookFile {
    #[serde(default)]
    hooks: Vec<HookConfig>,
}

#[derive(Debug, Serialize)]
struct AuditEntry<'a> {
    timestamp: chrono::DateTime<Utc>,
    session_id: SessionId,
    event: &'a HookEvent,
    command: &'a str,
    blocking: bool,
    result: &'a HookResult,
}

impl HookEngine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn list_hooks(&self, project_root: &Path) -> io::Result<Vec<HookConfig>> {
        load_hooks(project_root)
    }

    pub fn injected_context(&self, project_root: &Path) -> io::Result<String> {
        let hooks = load_hooks(project_root)?;
        Ok(hooks
            .into_iter()
            .filter(|hook| hook.event == HookEvent::PreSpawn)
            .filter_map(|hook| hook.inject_context)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"))
    }

    pub fn run_hooks(
        &self,
        session_id: SessionId,
        project_root: &Path,
        workdir: &Path,
        event: HookEvent,
    ) -> io::Result<HookRunOutcome> {
        let hooks = load_hooks(project_root)?;
        let mut results = Vec::new();
        let mut blocked = false;

        for (hook_index, hook) in hooks.into_iter().enumerate() {
            if hook.event != event {
                continue;
            }
            if self.is_debounced(session_id, &hook.event, hook_index, hook.min_interval_secs) {
                continue;
            }

            let result = run_shell_hook(workdir, &hook.command, hook.timeout_secs)?;
            append_audit_log(project_root, session_id, &hook, &result)?;
            self.record_run(session_id, &hook.event, hook_index);

            let failed = !result.success;
            results.push(result);
            if failed && hook.blocking {
                blocked = true;
                break;
            }
        }

        Ok(HookRunOutcome { results, blocked })
    }

    pub fn discover_project_root(path: &Path) -> io::Result<Option<PathBuf>> {
        let repo = match Repository::discover(path) {
            Ok(repo) => repo,
            Err(_) => return Ok(None),
        };
        let Some(workdir) = repo.workdir() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bare repositories are not supported: {}", path.display()),
            ));
        };
        Ok(Some(workdir.to_path_buf()))
    }

    fn is_debounced(
        &self,
        session_id: SessionId,
        event: &HookEvent,
        hook_index: usize,
        min_interval_secs: Option<u64>,
    ) -> bool {
        let Some(min_interval_secs) = min_interval_secs else {
            return false;
        };
        let Ok(last_run_at) = self.last_run_at.lock() else {
            return false;
        };
        let key = DebounceKey {
            session_id,
            event: event.clone(),
            hook_index,
        };
        last_run_at
            .get(&key)
            .is_some_and(|last| last.elapsed() < Duration::from_secs(min_interval_secs))
    }

    fn record_run(&self, session_id: SessionId, event: &HookEvent, hook_index: usize) {
        if let Ok(mut last_run_at) = self.last_run_at.lock() {
            last_run_at.insert(
                DebounceKey {
                    session_id,
                    event: event.clone(),
                    hook_index,
                },
                Instant::now(),
            );
        }
    }
}

fn load_hooks(project_root: &Path) -> io::Result<Vec<HookConfig>> {
    let path = project_root.join(HOOKS_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)?;
    let parsed: HookFile = toml::from_str(&contents).map_err(io::Error::other)?;
    Ok(parsed.hooks)
}

fn append_audit_log(
    project_root: &Path,
    session_id: SessionId,
    hook: &HookConfig,
    result: &HookResult,
) -> io::Result<()> {
    let path = project_root.join(AUDIT_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let entry = AuditEntry {
        timestamp: Utc::now(),
        session_id,
        event: &hook.event,
        command: &hook.command,
        blocking: hook.blocking,
        result,
    };
    serde_json::to_writer(&mut file, &entry).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.flush()
}

fn run_shell_hook(workdir: &Path, command: &str, timeout_secs: u64) -> io::Result<HookResult> {
    let started_at = Instant::now();
    let mut child = Command::new("sh")
        .args(["-lc", command])
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("hook stdout pipe missing"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("hook stderr pipe missing"))?;

    let stdout_handle = thread::spawn(move || read_pipe(stdout));
    let stderr_handle = thread::spawn(move || read_pipe(stderr));
    let timeout = Duration::from_secs(timeout_secs);
    let mut timed_out = false;

    let success = loop {
        if let Some(status) = child.try_wait()? {
            break status.success();
        }
        if started_at.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        thread::sleep(POLL_INTERVAL);
    };

    let stdout = stdout_handle
        .join()
        .map_err(|_| io::Error::other("hook stdout reader panicked"))??;
    let mut stderr = stderr_handle
        .join()
        .map_err(|_| io::Error::other("hook stderr reader panicked"))??;
    if timed_out {
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        stderr.push_str(&format!("hook timed out after {}s", timeout_secs));
    }

    Ok(HookResult {
        success,
        stdout,
        stderr,
        duration_ms: u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

fn read_pipe<R: Read>(mut pipe: R) -> io::Result<String> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use serde_json::Value;
    use tempfile::tempdir;

    use super::{load_hooks, HookEngine};
    use rist_shared::{HookEvent, SessionId};

    fn write_hooks(root: &Path, contents: &str) {
        let path = root.join(".ristretto");
        fs::create_dir_all(&path).expect("create hooks dir");
        fs::write(path.join("hooks.toml"), contents).expect("write hooks");
    }

    #[test]
    fn loads_hooks_from_toml() {
        let temp = tempdir().expect("tempdir");
        write_hooks(
            temp.path(),
            r#"
                [[hooks]]
                event = "pre_spawn"
                command = "echo before"
                blocking = true
                timeout_secs = 5
                inject_context = "Repo policy"

                [[hooks]]
                event = "post_output"
                command = "echo after"
                blocking = false
                timeout_secs = 3
                min_interval_secs = 60
            "#,
        );

        let hooks = load_hooks(temp.path()).expect("load hooks");
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0].event, HookEvent::PreSpawn);
        assert_eq!(hooks[1].min_interval_secs, Some(60));
    }

    #[test]
    fn pipeline_stops_on_failed_blocking_hook() {
        let temp = tempdir().expect("tempdir");
        write_hooks(
            temp.path(),
            r#"
                [[hooks]]
                event = "pre_merge"
                command = "printf 'first'; exit 1"
                blocking = true
                timeout_secs = 1

                [[hooks]]
                event = "pre_merge"
                command = "printf 'second' >> hook.out"
                blocking = false
                timeout_secs = 1
            "#,
        );

        let engine = HookEngine::new();
        let outcome = engine
            .run_hooks(
                SessionId::new(),
                temp.path(),
                temp.path(),
                HookEvent::PreMerge,
            )
            .expect("run hooks");

        assert!(outcome.blocked);
        assert_eq!(outcome.results.len(), 1);
        assert!(!temp.path().join("hook.out").exists());
    }

    #[test]
    fn failed_non_blocking_hook_does_not_stop_pipeline() {
        let temp = tempdir().expect("tempdir");
        write_hooks(
            temp.path(),
            r#"
                [[hooks]]
                event = "post_output"
                command = "printf 'first'; exit 1"
                blocking = false
                timeout_secs = 1

                [[hooks]]
                event = "post_output"
                command = "printf 'second' > hook.out"
                blocking = true
                timeout_secs = 1
            "#,
        );

        let engine = HookEngine::new();
        let outcome = engine
            .run_hooks(
                SessionId::new(),
                temp.path(),
                temp.path(),
                HookEvent::PostOutput,
            )
            .expect("run hooks");

        assert!(!outcome.blocked);
        assert_eq!(outcome.results.len(), 2);
        assert_eq!(
            fs::read_to_string(temp.path().join("hook.out")).expect("hook output"),
            "second"
        );
    }

    #[test]
    fn debounce_skips_repeated_execution_inside_interval() {
        let temp = tempdir().expect("tempdir");
        write_hooks(
            temp.path(),
            r#"
                [[hooks]]
                event = "post_output"
                command = "printf '.' >> count.out"
                blocking = false
                timeout_secs = 1
                min_interval_secs = 60
            "#,
        );

        let engine = HookEngine::new();
        let session_id = SessionId::new();
        let first = engine
            .run_hooks(session_id, temp.path(), temp.path(), HookEvent::PostOutput)
            .expect("first run");
        let second = engine
            .run_hooks(session_id, temp.path(), temp.path(), HookEvent::PostOutput)
            .expect("second run");

        assert_eq!(first.results.len(), 1);
        assert!(second.results.is_empty());
        assert_eq!(
            fs::read_to_string(temp.path().join("count.out")).expect("count"),
            "."
        );
    }

    #[test]
    fn inject_context_aggregates_in_file_order() {
        let temp = tempdir().expect("tempdir");
        write_hooks(
            temp.path(),
            r#"
                [[hooks]]
                event = "pre_spawn"
                command = "true"
                blocking = false
                timeout_secs = 1
                inject_context = "First layer"

                [[hooks]]
                event = "pre_spawn"
                command = "true"
                blocking = false
                timeout_secs = 1
                inject_context = "Second layer"
            "#,
        );

        let engine = HookEngine::new();
        let injected = engine
            .injected_context(temp.path())
            .expect("inject context");
        assert_eq!(injected, "First layer\n\nSecond layer");
    }

    #[test]
    fn audit_log_uses_jsonl_entries() {
        let temp = tempdir().expect("tempdir");
        write_hooks(
            temp.path(),
            r#"
                [[hooks]]
                event = "on_rotation"
                command = "printf 'rotated'"
                blocking = true
                timeout_secs = 1
            "#,
        );

        let engine = HookEngine::new();
        let session_id = SessionId::new();
        let outcome = engine
            .run_hooks(session_id, temp.path(), temp.path(), HookEvent::OnRotation)
            .expect("run hooks");
        assert_eq!(outcome.results.len(), 1);

        let audit = fs::read_to_string(temp.path().join(".ristretto/hook-audit.jsonl"))
            .expect("read audit");
        let line = audit.lines().next().expect("audit line");
        let entry: Value = serde_json::from_str(line).expect("valid json");
        let session_id_text = session_id.0.to_string();
        assert_eq!(
            entry.get("session_id").and_then(Value::as_str),
            Some(session_id_text.as_str())
        );
        assert_eq!(
            entry.get("command").and_then(Value::as_str),
            Some("printf 'rotated'")
        );
        assert_eq!(
            entry
                .get("result")
                .and_then(|result| result.get("stdout"))
                .and_then(Value::as_str),
            Some("rotated")
        );
    }
}
