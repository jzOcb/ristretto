//! Persistent session metadata storage.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rist_shared::AgentInfo;
use rist_shared::SessionId;
use serde::{Deserialize, Serialize};

/// Persisted daemon session store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStore {
    sessions: Vec<AgentInfo>,
    #[serde(skip)]
    path: PathBuf,
}

impl SessionStore {
    /// Creates an empty session store for the supplied path.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            sessions: Vec::new(),
            path,
        }
    }

    /// Loads a session store from disk, returning an empty store when missing.
    pub fn load(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::new(path.to_path_buf()));
        }
        let contents = fs::read_to_string(path)?;
        let sessions = serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Self {
            sessions,
            path: path.to_path_buf(),
        })
    }

    /// Saves the session store atomically using a temp file, `fsync`, and rename.
    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_path = unique_temp_path(&self.path);
        let payload = serde_json::to_vec_pretty(&self.sessions)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut file = fs::File::create(&temp_path)?;
        use std::io::Write as _;
        file.write_all(&payload)?;
        file.sync_all()?;
        fs::rename(temp_path, &self.path)
    }

    /// Adds a new session record.
    pub fn add(&mut self, session: AgentInfo) {
        self.sessions.push(session);
    }

    /// Updates a session record in place.
    pub fn update(&mut self, session: AgentInfo) {
        if let Some(existing) = self
            .sessions
            .iter_mut()
            .find(|candidate| candidate.id == session.id)
        {
            *existing = session;
        } else {
            self.sessions.push(session);
        }
    }

    /// Removes a session record.
    pub fn remove(&mut self, id: SessionId) {
        self.sessions.retain(|session| session.id != id);
    }

    /// Returns all stored sessions.
    #[must_use]
    pub fn sessions(&self) -> &[AgentInfo] {
        &self.sessions
    }
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session-store");
    let unique = format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    );
    path.with_file_name(unique)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use chrono::Utc;
    use tempfile::tempdir;

    use super::*;
    use rist_shared::{AgentStatus, AgentType, ContextUsage};

    fn sample_agent(task: &str) -> AgentInfo {
        AgentInfo {
            id: SessionId::new(),
            agent_type: AgentType::Codex,
            model: None,
            task: task.to_owned(),
            status: AgentStatus::Working,
            workdir: PathBuf::from("/tmp/worktree"),
            branch: Some("rist/test".to_owned()),
            file_ownership: vec![PathBuf::from("src/lib.rs")],
            created_at: Utc::now(),
            last_output_at: Some(Utc::now()),
            context_usage: Some(ContextUsage {
                estimated_tokens: 42,
                max_tokens: 128_000,
                percentage: 12.5,
            }),
            exit_code: None,
            metadata: HashMap::from([("source".to_owned(), "test".to_owned())]),
        }
    }

    #[test]
    fn save_and_load_roundtrip_preserves_sessions() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("sessions.json");
        let mut store = SessionStore::new(path.clone());
        let first = sample_agent("first task");
        let second = sample_agent("second task");

        store.add(first.clone());
        store.add(second.clone());
        store.save().expect("save");

        let loaded = SessionStore::load(&path).expect("load");
        assert_eq!(loaded.sessions(), &[first, second]);
        assert_eq!(loaded.path, path);
    }

    #[test]
    fn concurrent_saves_produce_valid_store_contents() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("sessions.json");
        let barrier = Arc::new(Barrier::new(8));
        let mut handles = Vec::new();

        for index in 0..8 {
            let barrier = Arc::clone(&barrier);
            let path = path.clone();
            handles.push(thread::spawn(move || {
                let mut store = SessionStore::new(path);
                store.add(sample_agent(&format!("task-{index}")));
                barrier.wait();
                store.save().expect("concurrent save");
            }));
        }

        for handle in handles {
            handle.join().expect("thread");
        }

        let raw = fs::read_to_string(&path).expect("saved file");
        let sessions: Vec<AgentInfo> = serde_json::from_str(&raw).expect("valid json");
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].task.starts_with("task-"));
    }

    #[test]
    fn serde_skip_omits_path_and_deserializes_default_path() {
        let path = PathBuf::from("/tmp/sessions.json");
        let mut store = SessionStore::new(path);
        store.add(sample_agent("serialize"));

        let value = serde_json::to_value(&store).expect("serialize");
        assert!(value.get("path").is_none());
        assert_eq!(
            value.get("sessions"),
            Some(&serde_json::to_value(store.sessions()).expect("sessions"))
        );

        let decoded: SessionStore = serde_json::from_value(value).expect("deserialize");
        assert_eq!(decoded.sessions(), store.sessions());
        assert!(decoded.path.as_os_str().is_empty());
    }

    #[test]
    fn load_returns_invalid_data_for_corrupt_json() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("sessions.json");
        fs::write(&path, "{ not valid json").expect("write");

        let error = SessionStore::load(&path).expect_err("corrupt data should fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn save_fails_when_parent_path_is_not_a_directory() {
        let dir = tempdir().expect("tempdir");
        let bad_parent = dir.path().join("not-a-directory");
        fs::write(&bad_parent, "blocking file").expect("write");

        let mut store = SessionStore::new(bad_parent.join("sessions.json"));
        store.add(sample_agent("bad path"));

        assert!(store.save().is_err());
    }
}
