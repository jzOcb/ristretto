//! Command output filtering and smart truncation.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rist_shared::{FilterConfig, FilterMode, FilterRule, OutputStats};

const CONFIG_DIR: &str = ".ristretto";
const CONFIG_FILE: &str = "filters.toml";

#[derive(Debug, Deserialize)]
struct RawFilterConfig {
    #[serde(default)]
    defaults: RawDefaults,
    #[serde(default)]
    filters: Vec<FilterRule>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDefaults {
    max_lines: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredOutput {
    pub stdout: String,
    pub stderr: String,
    pub stats: OutputStats,
}

#[derive(Debug, Clone, Default)]
pub struct OutputFilter {
    config: FilterConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandKind {
    CargoTest,
    CargoClippy,
    CargoBuild,
    GitLog,
    GitDiff,
    Unknown,
}

use serde::Deserialize;

impl OutputFilter {
    #[must_use]
    pub fn new(config: FilterConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn load_or_default(workdir: &Path) -> Self {
        Self::load(workdir).unwrap_or_default()
    }

    pub fn load(workdir: &Path) -> io::Result<Self> {
        let config = match find_config_path(workdir) {
            Some(path) => load_config_from_file(&path)?,
            None => FilterConfig::default(),
        };
        Ok(Self::new(config))
    }

    #[must_use]
    pub fn filter_command(
        &self,
        command: &str,
        stdout: &[u8],
        stderr: &[u8],
        exit_code: i32,
    ) -> FilteredOutput {
        let raw_stdout = String::from_utf8_lossy(stdout).into_owned();
        let raw_stderr = String::from_utf8_lossy(stderr).into_owned();
        let raw_bytes =
            u64::try_from(stdout.len().saturating_add(stderr.len())).unwrap_or(u64::MAX);

        let kind = command_kind(command);
        let rule = self.matching_rule(command);
        let mode = rule
            .and_then(|rule| rule.mode)
            .or_else(|| builtin_mode(kind))
            .unwrap_or(FilterMode::Tail);
        let max_lines = rule
            .and_then(|rule| rule.max_lines)
            .or_else(|| builtin_max_lines(kind))
            .unwrap_or(self.config.max_lines);

        let (stdout, stderr) = match mode {
            FilterMode::Smart => smart_filter(kind, &raw_stdout, &raw_stderr, exit_code, max_lines),
            FilterMode::Head => {
                let stdout = if kind == CommandKind::GitLog {
                    truncate_git_log(&raw_stdout, max_lines)
                } else {
                    truncate_head(&raw_stdout, max_lines)
                };
                (stdout, truncate_head(&raw_stderr, max_lines))
            }
            FilterMode::Tail => (
                truncate_tail(&raw_stdout, max_lines),
                truncate_tail(&raw_stderr, max_lines),
            ),
            FilterMode::None => (raw_stdout.clone(), raw_stderr.clone()),
        };

        let filtered_bytes =
            u64::try_from(stdout.len().saturating_add(stderr.len())).unwrap_or(u64::MAX);
        FilteredOutput {
            stdout,
            stderr,
            stats: OutputStats {
                raw_bytes,
                filtered_bytes,
                filter_applied: filtered_bytes != raw_bytes,
            },
        }
    }

    fn matching_rule(&self, command: &str) -> Option<&FilterRule> {
        self.config
            .filters
            .iter()
            .find(|rule| glob_matches(&rule.pattern, command))
    }
}

fn load_config_from_file(path: &Path) -> io::Result<FilterConfig> {
    let contents = fs::read_to_string(path)?;
    let raw: RawFilterConfig = toml::from_str(&contents).map_err(io::Error::other)?;
    let mut config = FilterConfig::default();
    if let Some(max_lines) = raw.defaults.max_lines {
        config.max_lines = max_lines;
    }
    if !raw.filters.is_empty() {
        let mut filters = raw.filters;
        filters.extend(config.filters);
        config.filters = filters;
    }
    Ok(config)
}

fn find_config_path(workdir: &Path) -> Option<PathBuf> {
    for dir in workdir.ancestors() {
        let candidate = dir.join(CONFIG_DIR).join(CONFIG_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn command_kind(command: &str) -> CommandKind {
    let normalized = command.trim();
    if normalized.starts_with("cargo test") {
        CommandKind::CargoTest
    } else if normalized.starts_with("cargo clippy") {
        CommandKind::CargoClippy
    } else if normalized.starts_with("cargo build") {
        CommandKind::CargoBuild
    } else if normalized.starts_with("git log") {
        CommandKind::GitLog
    } else if normalized.starts_with("git diff") {
        CommandKind::GitDiff
    } else {
        CommandKind::Unknown
    }
}

fn builtin_mode(kind: CommandKind) -> Option<FilterMode> {
    match kind {
        CommandKind::CargoTest | CommandKind::CargoClippy | CommandKind::CargoBuild => {
            Some(FilterMode::Smart)
        }
        CommandKind::GitLog | CommandKind::GitDiff => Some(FilterMode::Head),
        CommandKind::Unknown => None,
    }
}

fn builtin_max_lines(kind: CommandKind) -> Option<usize> {
    match kind {
        CommandKind::GitLog => Some(20),
        CommandKind::GitDiff => Some(200),
        _ => None,
    }
}

fn smart_filter(
    kind: CommandKind,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
    max_lines: usize,
) -> (String, String) {
    match kind {
        CommandKind::CargoTest => smart_cargo_test(stdout, stderr, exit_code, max_lines),
        CommandKind::CargoClippy => smart_cargo_clippy(stdout, stderr, exit_code),
        CommandKind::CargoBuild => smart_cargo_build(stdout, stderr, exit_code),
        CommandKind::GitLog => (truncate_git_log(stdout, max_lines), stderr.to_owned()),
        CommandKind::GitDiff => (truncate_head(stdout, max_lines), stderr.to_owned()),
        CommandKind::Unknown => (
            truncate_tail(stdout, max_lines),
            truncate_tail(stderr, max_lines),
        ),
    }
}

fn smart_cargo_test(
    stdout: &str,
    stderr: &str,
    exit_code: i32,
    max_lines: usize,
) -> (String, String) {
    if exit_code == 0 {
        let summary = extract_cargo_test_summary(stdout)
            .or_else(|| extract_cargo_test_summary(stderr))
            .unwrap_or_else(|| "✓ tests passed".to_owned());
        return (summary, String::new());
    }

    let lines = stdout
        .lines()
        .filter(|line| keep_cargo_test_failure_line(line))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let filtered_stdout = if lines.is_empty() {
        truncate_tail(stdout, max_lines)
    } else {
        lines.join("\n")
    };
    (filtered_stdout, stderr.to_owned())
}

fn smart_cargo_clippy(stdout: &str, stderr: &str, exit_code: i32) -> (String, String) {
    let combined = format!("{stdout}\n{stderr}");
    if exit_code == 0 && !combined.to_ascii_lowercase().contains("warning:") {
        ("✓ no warnings".to_owned(), String::new())
    } else {
        (stdout.to_owned(), stderr.to_owned())
    }
}

fn smart_cargo_build(stdout: &str, stderr: &str, exit_code: i32) -> (String, String) {
    if exit_code == 0 {
        ("✓ compiled successfully".to_owned(), String::new())
    } else {
        (stdout.to_owned(), stderr.to_owned())
    }
}

fn extract_cargo_test_summary(text: &str) -> Option<String> {
    for line in text.lines() {
        if !line.contains("test result:") {
            continue;
        }
        let passed = extract_count(line, "passed")?;
        let failed = extract_count(line, "failed").unwrap_or(0);
        let duration = extract_duration(line).unwrap_or_else(|| "?s".to_owned());
        return Some(format!("✓ {passed} passed, {failed} failed ({duration})"));
    }
    None
}

fn keep_cargo_test_failure_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("test ") && lower.ends_with(" ... ok") {
        return false;
    }
    lower.contains("fail")
        || lower.contains("error")
        || lower.contains("panic")
        || lower.contains("backtrace")
        || lower.contains("stack backtrace")
        || lower.contains("test result:")
        || lower.contains("running ")
        || lower.contains("note:")
}

fn extract_count(line: &str, suffix: &str) -> Option<u64> {
    let target = format!(" {suffix}");
    for segment in line.split([',', ';']) {
        let segment = segment.trim();
        if segment.ends_with(&target) {
            let value = segment
                .split_whitespace()
                .find(|part| part.chars().all(|ch| ch.is_ascii_digit()))?;
            return value.parse().ok();
        }
    }
    None
}

fn extract_duration(line: &str) -> Option<String> {
    line.split("finished in ")
        .nth(1)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn truncate_head(text: &str, max_lines: usize) -> String {
    truncate_lines(text, max_lines, false)
}

fn truncate_tail(text: &str, max_lines: usize) -> String {
    truncate_lines(text, max_lines, true)
}

fn truncate_lines(text: &str, max_lines: usize, tail: bool) -> String {
    let lines = text.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    if lines.len() <= max_lines || max_lines == 0 {
        return text.to_owned();
    }

    let kept = if tail {
        lines[lines.len().saturating_sub(max_lines)..].to_vec()
    } else {
        lines[..max_lines].to_vec()
    };
    let omitted = lines.len().saturating_sub(kept.len());
    let notice = format!("[... truncated {omitted} lines ...]");
    if tail {
        let mut output = String::new();
        output.push_str(&notice);
        output.push('\n');
        output.push_str(&kept.join("\n"));
        output
    } else {
        let mut output = kept.join("\n");
        output.push('\n');
        output.push_str(&notice);
        output
    }
}

fn truncate_git_log(text: &str, max_entries: usize) -> String {
    if max_entries == 0 {
        return String::new();
    }

    let lines = text.lines().collect::<Vec<_>>();
    let mut kept = Vec::new();
    let mut commits = 0usize;
    for line in &lines {
        if line.starts_with("commit ") {
            commits += 1;
            if commits > max_entries {
                break;
            }
        }
        kept.push((*line).to_owned());
    }

    if kept.len() == lines.len() {
        return text.to_owned();
    }

    let omitted = commits.saturating_sub(max_entries);
    let mut output = kept.join("\n");
    output.push('\n');
    output.push_str(&format!("[... truncated {omitted} git log entries ...]"));
    output
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut p, mut t) = (0usize, 0usize);
    let mut star = None;
    let mut star_text = 0usize;

    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            star_text = t;
        } else if let Some(star_pos) = star {
            p = star_pos + 1;
            star_text += 1;
            t = star_text;
        } else {
            return false;
        }
    }

    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{load_config_from_file, OutputFilter};

    #[test]
    fn cargo_test_success_is_summarized() {
        let filter = OutputFilter::default();
        let output = filter.filter_command(
            "cargo test",
            b"running 2 tests\n...\ntest result: ok. 2 passed; 0 failed; finished in 0.32s\n",
            b"",
            0,
        );
        assert_eq!(output.stdout, "✓ 2 passed, 0 failed (0.32s)");
        assert!(output.stderr.is_empty());
        assert!(output.stats.filter_applied);
    }

    #[test]
    fn cargo_test_failure_keeps_failures_and_backtrace() {
        let filter = OutputFilter::default();
        let output = filter.filter_command(
            "cargo test",
            b"running 3 tests\ntest ok_case ... ok\ntest sad_case ... FAILED\nfailures:\n sad_case\nstack backtrace:\n  0: foo\n",
            b"thread 'sad_case' panicked at src/lib.rs:1\n",
            101,
        );
        assert!(!output.stdout.contains("ok_case"));
        assert!(output.stdout.contains("sad_case"));
        assert!(output.stdout.contains("stack backtrace"));
        assert!(output.stderr.contains("panicked"));
    }

    #[test]
    fn cargo_clippy_success_is_summarized() {
        let filter = OutputFilter::default();
        let output = filter.filter_command("cargo clippy", b"", b"", 0);
        assert_eq!(output.stdout, "✓ no warnings");
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn default_fallback_keeps_tail_lines() {
        let filter = OutputFilter::default();
        let stdout = (1..=205)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let output = filter.filter_command("unknown command", stdout.as_bytes(), b"", 0);
        assert!(output.stdout.contains("[... truncated 5 lines ...]"));
        assert!(output.stdout.contains("line 205"));
        assert!(!output.stdout.contains("line 1\n"));
    }

    #[test]
    fn filter_config_loading_merges_defaults() {
        let dir = tempdir().expect("tempdir");
        let config_dir = dir.path().join(".ristretto");
        fs::create_dir_all(&config_dir).expect("config dir");
        let path = config_dir.join("filters.toml");
        fs::write(
            &path,
            r#"
[defaults]
max_lines = 42

[[filters]]
pattern = "custom *"
mode = "none"
"#,
        )
        .expect("config");

        let config = load_config_from_file(&path).expect("config should load");
        assert_eq!(config.max_lines, 42);
        assert_eq!(config.filters[0].pattern, "custom *");
        assert!(config
            .filters
            .iter()
            .any(|rule| rule.pattern == "cargo test*"));
    }
}
