//! In-memory file ownership tracking for agent sessions.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rist_shared::SessionId;
use serde::{Deserialize, Serialize};

/// Tracks which session currently owns each file path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileOwnership {
    ownership: HashMap<PathBuf, SessionId>,
    interfaces: HashMap<PathBuf, InterfaceContract>,
}

/// Declared cross-agent contract for a shared interface file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceContract {
    /// Session that owns the interface.
    pub owner: SessionId,
    /// Human-readable contract summary.
    pub contract: String,
    /// Sessions that consume the interface.
    pub consumers: Vec<SessionId>,
}

/// Ownership collision encountered while declaring files for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipConflict {
    /// File that is already owned.
    pub file: PathBuf,
    /// Session that currently owns the file.
    pub current_owner: SessionId,
    /// Session requesting ownership.
    pub requested_by: SessionId,
}

impl FileOwnership {
    /// Creates an empty ownership registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares ownership of `files` for `session_id`.
    pub fn declare(
        &mut self,
        session_id: SessionId,
        files: Vec<PathBuf>,
    ) -> Result<(), OwnershipConflict> {
        for file in &files {
            if let Some(current_owner) = self.ownership.get(file).copied() {
                if current_owner != session_id {
                    return Err(OwnershipConflict {
                        file: file.clone(),
                        current_owner,
                        requested_by: session_id,
                    });
                }
            }
        }

        for file in files {
            self.ownership.insert(file, session_id);
        }

        Ok(())
    }

    /// Releases every file and interface owned by `session_id`.
    pub fn release(&mut self, session_id: SessionId) {
        self.ownership.retain(|_, owner| *owner != session_id);
        self.interfaces
            .retain(|_, contract| contract.owner != session_id);
    }

    /// Returns whether `session_id` owns `file`.
    #[must_use]
    pub fn check(&self, session_id: SessionId, file: &Path) -> bool {
        self.ownership
            .get(file)
            .is_some_and(|owner| *owner == session_id)
    }

    /// Returns the current path-to-owner map.
    #[must_use]
    pub fn map(&self) -> &HashMap<PathBuf, SessionId> {
        &self.ownership
    }

    /// Saves the ownership registry to a JSON file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_vec_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, payload)
    }

    /// Loads an ownership registry from JSON.
    pub fn load(path: &Path) -> io::Result<Self> {
        let contents = fs::read(path)?;
        serde_json::from_slice(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

impl fmt::Display for OwnershipConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} is already owned by {}",
            self.file.display(),
            self.current_owner.0
        )
    }
}

impl std::error::Error for OwnershipConflict {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::FileOwnership;
    use rist_shared::SessionId;

    #[test]
    fn declare_and_release_roundtrip() {
        let mut ownership = FileOwnership::new();
        let session_id = SessionId::new();
        let file = PathBuf::from("src/lib.rs");

        ownership
            .declare(session_id, vec![file.clone()])
            .expect("declare");
        assert!(ownership.check(session_id, &file));

        ownership.release(session_id);
        assert!(!ownership.check(session_id, &file));
    }

    #[test]
    fn declare_rejects_conflicts() {
        let mut ownership = FileOwnership::new();
        let first = SessionId::new();
        let second = SessionId::new();
        let file = PathBuf::from("src/main.rs");

        ownership
            .declare(first, vec![file.clone()])
            .expect("declare");
        let conflict = ownership
            .declare(second, vec![file.clone()])
            .expect_err("conflict");

        assert_eq!(conflict.file, file);
        assert_eq!(conflict.current_owner, first);
        assert_eq!(conflict.requested_by, second);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let mut ownership = FileOwnership::new();
        let session_id = SessionId::new();
        ownership
            .declare(session_id, vec![PathBuf::from("src/ui.rs")])
            .expect("declare");

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("ownership.json");
        ownership.save(&path).expect("save");

        let loaded = FileOwnership::load(&path).expect("load");
        assert_eq!(loaded.map(), ownership.map());
    }
}
