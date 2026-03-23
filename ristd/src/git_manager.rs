//! Git worktree lifecycle and merge helpers for agent sessions.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use git2::{BranchType, Repository, WorktreeAddOptions};
use rist_shared::SessionId;

/// Stateless helper for agent git operations.
pub struct GitManager;

/// Preview of a branch merge into the current repository HEAD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergePreview {
    /// Unified diff between the current branch and the agent branch.
    pub diff: String,
    /// Number of files changed in the diff.
    pub files_changed: usize,
    /// Number of inserted lines.
    pub insertions: usize,
    /// Number of deleted lines.
    pub deletions: usize,
    /// Files with detected merge conflicts.
    pub conflicts: Vec<String>,
}

/// Result of a squash merge attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    /// Whether the merge completed successfully.
    pub success: bool,
    /// Human-readable outcome message.
    pub message: String,
    /// Commit hash created by the merge, when successful.
    pub commit_hash: Option<String>,
}

impl GitManager {
    /// Creates a new agent worktree rooted under `.ristretto/worktrees/<session-id>/`.
    pub fn create_worktree(
        repo_path: &Path,
        session_id: SessionId,
        task: &str,
    ) -> io::Result<PathBuf> {
        let repo = open_repo(repo_path)?;
        let short_id = short_session_id(session_id);
        let branch_name = branch_name(session_id, task);
        let worktree_path = repo
            .workdir()
            .unwrap_or(repo_path)
            .join(".ristretto")
            .join("worktrees")
            .join(&short_id);

        if let Some(parent) = worktree_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if worktree_path.exists() {
            fs::remove_dir_all(&worktree_path)?;
        }

        let head_commit = repo
            .head()
            .and_then(|head| head.peel_to_commit())
            .map_err(io::Error::other)?;
        repo.branch(&branch_name, &head_commit, false)
            .map_err(io::Error::other)?;

        let reference = repo
            .find_reference(&format!("refs/heads/{branch_name}"))
            .map_err(io::Error::other)?;
        let mut options = WorktreeAddOptions::new();
        options.reference(Some(&reference));
        repo.worktree(&short_id, &worktree_path, Some(&options))
            .map_err(io::Error::other)?;

        Ok(worktree_path)
    }

    /// Removes a worktree and optionally deletes its branch.
    pub fn remove_worktree(
        repo_path: &Path,
        worktree_path: &Path,
        delete_branch: bool,
    ) -> io::Result<()> {
        let repo = open_repo(repo_path)?;
        let branch = Repository::open(worktree_path)
            .ok()
            .and_then(|worktree_repo| {
                worktree_repo.head().ok().and_then(|head| {
                    if head.is_branch() {
                        head.shorthand().map(ToOwned::to_owned)
                    } else {
                        None
                    }
                })
            });

        let status = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(worktree_path)
            .current_dir(repo.workdir().unwrap_or(repo_path))
            .status()?;
        if !status.success() {
            return Err(io::Error::other(format!("git exited with status {status}")));
        }

        if delete_branch {
            if let Some(branch_name) = branch {
                let mut branch = repo
                    .find_branch(&branch_name, BranchType::Local)
                    .map_err(io::Error::other)?;
                branch.delete().map_err(io::Error::other)?;
            }
        }

        Ok(())
    }

    /// Returns a diff and summary for merging `branch` into the current branch.
    pub fn preview_merge(repo_path: &Path, branch: &str) -> io::Result<MergePreview> {
        let repo = open_repo(repo_path)?;
        let diff = run_git_capture(
            repo.workdir().unwrap_or(repo_path),
            ["diff", "HEAD", branch],
        )?;
        let stats = run_git_capture(
            repo.workdir().unwrap_or(repo_path),
            ["diff", "--shortstat", "HEAD", branch],
        )?;
        let conflicts = Self::detect_conflicts(repo_path, branch)?;
        let (files_changed, insertions, deletions) = parse_shortstat(&stats);

        Ok(MergePreview {
            diff,
            files_changed,
            insertions,
            deletions,
            conflicts,
        })
    }

    /// Squash-merges `branch` into the current branch and commits the result.
    pub fn squash_merge(repo_path: &Path, branch: &str, message: &str) -> io::Result<MergeResult> {
        let repo = open_repo(repo_path)?;
        let conflicts = Self::detect_conflicts(repo_path, branch)?;
        if !conflicts.is_empty() {
            return Ok(MergeResult {
                success: false,
                message: format!("merge conflicts detected: {}", conflicts.join(", ")),
                commit_hash: None,
            });
        }

        let workdir = repo.workdir().unwrap_or(repo_path);
        run_git(workdir, ["merge", "--squash", branch])?;
        match run_git(workdir, ["commit", "-m", message]) {
            Ok(()) => {
                let commit_hash = run_git_capture(workdir, ["rev-parse", "HEAD"])?;
                Ok(MergeResult {
                    success: true,
                    message: "squash merge completed".to_owned(),
                    commit_hash: Some(commit_hash.trim().to_owned()),
                })
            }
            Err(error) => {
                let _ = run_git(workdir, ["merge", "--abort"]);
                Err(error)
            }
        }
    }

    /// Detects merge conflicts for `branch` without updating the working tree.
    pub fn detect_conflicts(repo_path: &Path, branch: &str) -> io::Result<Vec<String>> {
        let repo = open_repo(repo_path)?;
        let workdir = repo.workdir().unwrap_or(repo_path);
        let base = run_git_capture(workdir, ["merge-base", "HEAD", branch])?;
        let output = run_git_capture(workdir, ["merge-tree", base.trim(), "HEAD", branch])?;
        let mut conflicts = Vec::new();
        let mut in_conflict = false;

        for line in output.lines() {
            let trimmed = line.trim_start();
            if line == "changed in both" {
                in_conflict = true;
                continue;
            }
            if in_conflict && line.starts_with("@@") {
                in_conflict = false;
                continue;
            }
            if !in_conflict {
                continue;
            }

            let marker = trimmed
                .strip_prefix("our ")
                .or_else(|| trimmed.strip_prefix("their "));
            let Some(entry) = marker else {
                continue;
            };
            let Some(path) = entry.split_whitespace().last() else {
                continue;
            };
            if !conflicts.iter().any(|existing| existing == path) {
                conflicts.push(path.to_owned());
            }
        }

        Ok(conflicts)
    }

    /// Generates a URL-friendly task slug for worktree branch names.
    fn task_slug(task: &str) -> String {
        let slug = task
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>();
        slug.split('-')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join("-")
            .chars()
            .take(30)
            .collect::<String>()
            .trim_matches('-')
            .to_owned()
    }
}

fn open_repo(repo_path: &Path) -> io::Result<Repository> {
    Repository::discover(repo_path).map_err(io::Error::other)
}

fn branch_name(session_id: SessionId, task: &str) -> String {
    format!(
        "rist/{}-{}",
        short_session_id(session_id),
        GitManager::task_slug(task)
    )
}

fn short_session_id(session_id: SessionId) -> String {
    session_id.0.simple().to_string().chars().take(8).collect()
}

fn parse_shortstat(stats: &str) -> (usize, usize, usize) {
    let mut numbers = stats
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<usize>().ok());
    (
        numbers.next().unwrap_or(0),
        numbers.next().unwrap_or(0),
        numbers.next().unwrap_or(0),
    )
}

fn run_git<I, S>(repo_path: &Path, args: I) -> io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("git exited with status {status}")))
    }
}

fn run_git_capture<I, S>(repo_path: &Path, args: I) -> io::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use git2::{Repository, RepositoryInitOptions, Signature};
    use tempfile::tempdir;

    use super::GitManager;
    use rist_shared::SessionId;

    fn init_repo() -> (tempfile::TempDir, Repository) {
        let dir = tempdir().expect("tempdir");
        let mut options = RepositoryInitOptions::new();
        options.initial_head("main");
        let repo = Repository::init_opts(dir.path(), &options).expect("repo");
        {
            let mut config = repo.config().expect("config");
            config.set_str("user.name", "Ristretto").expect("user");
            config
                .set_str("user.email", "ristretto@example.com")
                .expect("email");
        }
        fs::write(dir.path().join("README.md"), "hello\n").expect("write");
        let mut index = repo.index().expect("index");
        index.add_path(Path::new("README.md")).expect("add");
        let tree_id = index.write_tree().expect("tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let sig = Signature::now("Ristretto", "ristretto@example.com").expect("sig");
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .expect("commit");
        drop(tree);
        (dir, repo)
    }

    fn git(repo_path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .status()
            .expect("git command");
        assert!(status.success(), "git {:?} failed with {status}", args);
    }

    fn checkout(repo_path: &Path, branch: &str) {
        git(repo_path, &["checkout", "-f", branch]);
        git(repo_path, &["clean", "-fd"]);
    }

    fn create_branch(repo: &Repository, branch: &str) {
        let head = repo.head().expect("head");
        let commit = head.peel_to_commit().expect("commit");
        repo.branch(branch, &commit, false).expect("branch");
    }

    fn commit_file(repo: &Repository, path: &Path, rel_path: &str, contents: &str, message: &str) {
        fs::write(path.join(rel_path), contents).expect("write");
        let mut index = repo.index().expect("index");
        index.add_path(Path::new(rel_path)).expect("add");
        let tree_id = index.write_tree().expect("tree");
        let tree = repo.find_tree(tree_id).expect("tree");
        let sig = Signature::now("Ristretto", "ristretto@example.com").expect("sig");
        let parent = repo
            .head()
            .expect("head")
            .peel_to_commit()
            .expect("parent commit");
        let commit_id = repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
            .expect("commit");
        drop(tree);
        let _ = repo.find_commit(commit_id).expect("commit");
    }

    #[test]
    fn task_slug_normalizes_and_truncates() {
        let slug = GitManager::task_slug("Build Ristretto Phase 2A!!! with spaces");
        assert_eq!(slug, "build-ristretto-phase-2a-with");
        assert!(slug.len() <= 30);
    }

    #[test]
    fn create_and_remove_worktree() {
        let (dir, _repo) = init_repo();
        let session_id = SessionId::new();
        let worktree = GitManager::create_worktree(dir.path(), session_id, "Implement merge flow")
            .expect("create worktree");
        assert!(worktree.exists());
        assert!(worktree.join(".git").exists());

        GitManager::remove_worktree(dir.path(), &worktree, true).expect("remove");
        assert!(!worktree.exists());
    }

    #[test]
    fn detect_conflicts_returns_conflicted_paths() {
        let (dir, repo) = init_repo();
        create_branch(&repo, "feature");

        checkout(dir.path(), "feature");
        commit_file(
            &repo,
            dir.path(),
            "README.md",
            "conflict\nfeature\n",
            "feature",
        );

        checkout(dir.path(), "main");
        commit_file(&repo, dir.path(), "README.md", "conflict\nmain\n", "main");

        let conflicts = GitManager::detect_conflicts(dir.path(), "feature").expect("conflicts");
        assert_eq!(conflicts, vec!["README.md".to_owned()]);
    }

    #[test]
    fn detect_conflicts_returns_empty_for_clean_merge() {
        let (dir, repo) = init_repo();
        create_branch(&repo, "feature");

        checkout(dir.path(), "feature");
        commit_file(
            &repo,
            dir.path(),
            "feature.txt",
            "hello from feature\n",
            "feature",
        );
        checkout(dir.path(), "main");

        let conflicts = GitManager::detect_conflicts(dir.path(), "feature").expect("conflicts");
        assert!(conflicts.is_empty());
    }

    #[test]
    fn squash_merge_succeeds_for_clean_branch() {
        let (dir, repo) = init_repo();
        create_branch(&repo, "feature");

        checkout(dir.path(), "feature");
        commit_file(
            &repo,
            dir.path(),
            "feature.txt",
            "hello from feature\n",
            "feature work",
        );
        checkout(dir.path(), "main");

        let result =
            GitManager::squash_merge(dir.path(), "feature", "squash feature").expect("merge");

        assert!(result.success);
        assert_eq!(result.message, "squash merge completed");
        assert!(result.commit_hash.is_some());
        assert_eq!(
            fs::read_to_string(dir.path().join("feature.txt")).expect("merged file"),
            "hello from feature\n"
        );

        let head = repo.head().expect("head").peel_to_commit().expect("commit");
        assert_eq!(head.message().map(str::trim), Some("squash feature"));
    }

    #[test]
    fn preview_merge_reports_diff_and_stats() {
        let (dir, repo) = init_repo();
        create_branch(&repo, "feature");

        checkout(dir.path(), "feature");
        commit_file(
            &repo,
            dir.path(),
            "feature.txt",
            "line one\nline two\n",
            "feature work",
        );
        checkout(dir.path(), "main");

        let preview = GitManager::preview_merge(dir.path(), "feature").expect("preview");
        assert!(preview.diff.contains("diff --git"));
        assert!(preview.diff.contains("feature.txt"));
        assert!(preview.diff.contains("+line one"));
        assert_eq!(preview.files_changed, 1);
        assert_eq!(preview.insertions, 2);
        assert_eq!(preview.deletions, 0);
        assert!(preview.conflicts.is_empty());
    }
}
