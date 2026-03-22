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
        let temp_path = self.path.with_extension("json.tmp");
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
