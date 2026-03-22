//! Persistent task-planning state and dependency queries.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rist_shared::{SessionId, Task, TaskGraph, TaskStatus};

/// Planner-owned task graph persistence and scheduling queries.
#[derive(Debug, Clone)]
pub struct TaskPlanner {
    task_graph: TaskGraph,
    state_path: PathBuf,
}

impl TaskPlanner {
    /// Creates a planner backed by `state_path`.
    #[must_use]
    pub fn new(state_path: PathBuf) -> Self {
        Self {
            task_graph: TaskGraph {
                tasks: Vec::new(),
                updated_at: Utc::now(),
            },
            state_path,
        }
    }

    /// Loads persisted task graph state from disk.
    pub fn load(&mut self) -> io::Result<()> {
        if !self.state_path.exists() {
            return Ok(());
        }
        let contents = fs::read_to_string(&self.state_path)?;
        self.task_graph = serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(())
    }

    /// Saves current task graph state to disk.
    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_path = unique_temp_path(&self.state_path);
        let payload = serde_json::to_vec_pretty(&self.task_graph)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut file = fs::File::create(&temp_path)?;
        use std::io::Write as _;
        file.write_all(&payload)?;
        file.sync_all()?;
        fs::rename(temp_path, &self.state_path)
    }

    /// Returns the current task graph.
    #[must_use]
    pub fn graph(&self) -> &TaskGraph {
        &self.task_graph
    }

    /// Replaces the entire task graph.
    pub fn set_graph(&mut self, graph: TaskGraph) -> io::Result<()> {
        self.task_graph = graph;
        self.task_graph.updated_at = Utc::now();
        self.save()
    }

    /// Updates a single task's status.
    pub fn update_task_status(&mut self, task_id: &str, status: TaskStatus) -> io::Result<()> {
        let task = self
            .task_graph
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task not found"))?;
        task.status = status;
        self.task_graph.updated_at = Utc::now();
        self.save()
    }

    /// Returns tasks that are ready to execute.
    #[must_use]
    pub fn ready_tasks(&self) -> Vec<&Task> {
        self.task_graph
            .tasks
            .iter()
            .filter(|task| {
                task.status == TaskStatus::Pending && self.unsatisfied_deps(task).is_empty()
            })
            .collect()
    }

    /// Returns tasks blocked on dependencies.
    #[must_use]
    pub fn blocked_tasks(&self) -> Vec<(&Task, Vec<&str>)> {
        self.task_graph
            .tasks
            .iter()
            .filter_map(|task| {
                let missing = self.unsatisfied_deps(task);
                if missing.is_empty() {
                    None
                } else {
                    Some((task, missing))
                }
            })
            .collect()
    }

    /// Assigns an agent to a task.
    pub fn assign_task(&mut self, task_id: &str, agent_id: SessionId) -> io::Result<()> {
        let task = self
            .task_graph
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "task not found"))?;
        task.owner = Some(agent_id);
        self.task_graph.updated_at = Utc::now();
        self.save()
    }

    /// Returns planner summary statistics.
    #[must_use]
    pub fn stats(&self) -> PlannerStats {
        let blocked = self.blocked_tasks().len();
        let mut stats = PlannerStats {
            total: self.task_graph.tasks.len(),
            pending: 0,
            working: 0,
            done: 0,
            blocked,
            error: 0,
        };

        for task in &self.task_graph.tasks {
            match task.status {
                TaskStatus::Pending => stats.pending += 1,
                TaskStatus::Assigned | TaskStatus::Working | TaskStatus::Review => {
                    stats.working += 1
                }
                TaskStatus::Done => stats.done += 1,
                TaskStatus::Blocked => stats.error += 1,
                TaskStatus::Unknown => stats.error += 1,
            }
        }

        stats
    }

    fn unsatisfied_deps<'a>(&'a self, task: &'a Task) -> Vec<&'a str> {
        let done = self
            .task_graph
            .tasks
            .iter()
            .filter(|candidate| candidate.status == TaskStatus::Done)
            .map(|candidate| candidate.id.as_str())
            .collect::<HashSet<_>>();

        task.depends_on
            .iter()
            .filter_map(|dep| (!done.contains(dep.as_str())).then_some(dep.as_str()))
            .collect()
    }
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("task-graph");
    let unique = format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    );
    path.with_file_name(unique)
}

/// Aggregate task-graph statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannerStats {
    /// Total tasks in the graph.
    pub total: usize,
    /// Pending tasks.
    pub pending: usize,
    /// Assigned, working, or review tasks.
    pub working: usize,
    /// Completed tasks.
    pub done: usize,
    /// Tasks blocked on dependencies.
    pub blocked: usize,
    /// Tasks in an error-like state.
    pub error: usize,
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use rist_shared::{Priority, Task, TaskGraph};

    use super::*;

    fn task(id: &str, status: TaskStatus, depends_on: &[&str]) -> Task {
        Task {
            id: id.to_owned(),
            title: format!("Task {id}"),
            description: None,
            status,
            priority: Priority::Medium,
            agent_type: None,
            owner: None,
            depends_on: depends_on.iter().map(|dep| (*dep).to_owned()).collect(),
            file_ownership: Vec::new(),
        }
    }

    #[test]
    fn ready_tasks_only_include_satisfied_dependencies() {
        let temp = tempdir().expect("tempdir");
        let mut planner = TaskPlanner::new(temp.path().join("task_graph.json"));
        planner.task_graph = TaskGraph {
            tasks: vec![
                task("t1", TaskStatus::Done, &[]),
                task("t2", TaskStatus::Pending, &["t1"]),
                task("t3", TaskStatus::Pending, &["missing"]),
            ],
            updated_at: Utc::now(),
        };

        let ready = planner.ready_tasks();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "t2");
    }

    #[test]
    fn blocked_tasks_report_missing_dependencies() {
        let temp = tempdir().expect("tempdir");
        let mut planner = TaskPlanner::new(temp.path().join("task_graph.json"));
        planner.task_graph = TaskGraph {
            tasks: vec![task("t1", TaskStatus::Pending, &["t0"])],
            updated_at: Utc::now(),
        };

        let blocked = planner.blocked_tasks();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].0.id, "t1");
        assert_eq!(blocked[0].1, vec!["t0"]);
    }

    #[test]
    fn update_task_status_persists_changes() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("task_graph.json");
        let mut planner = TaskPlanner::new(path.clone());
        planner.task_graph = TaskGraph {
            tasks: vec![task("t1", TaskStatus::Pending, &[])],
            updated_at: Utc::now(),
        };

        planner
            .update_task_status("t1", TaskStatus::Done)
            .expect("update");

        let mut reloaded = TaskPlanner::new(path);
        reloaded.load().expect("load");
        assert_eq!(reloaded.graph().tasks[0].status, TaskStatus::Done);
    }

    #[test]
    fn save_load_roundtrip() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("task_graph.json");
        let original = TaskGraph {
            tasks: vec![task("t1", TaskStatus::Assigned, &[])],
            updated_at: Utc::now(),
        };
        let mut planner = TaskPlanner::new(path.clone());
        planner.set_graph(original).expect("save");

        let mut loaded = TaskPlanner::new(path);
        loaded.load().expect("load");
        assert_eq!(loaded.graph().tasks.len(), 1);
        assert_eq!(loaded.graph().tasks[0].id, "t1");
        assert_eq!(loaded.graph().tasks[0].status, TaskStatus::Assigned);
    }

    #[test]
    fn stats_compute_expected_counts() {
        let temp = tempdir().expect("tempdir");
        let mut planner = TaskPlanner::new(temp.path().join("task_graph.json"));
        planner.task_graph = TaskGraph {
            tasks: vec![
                task("t1", TaskStatus::Pending, &[]),
                task("t2", TaskStatus::Assigned, &[]),
                task("t3", TaskStatus::Working, &[]),
                task("t4", TaskStatus::Review, &[]),
                task("t5", TaskStatus::Done, &[]),
                task("t6", TaskStatus::Blocked, &["t7"]),
            ],
            updated_at: Utc::now(),
        };

        let stats = planner.stats();
        assert_eq!(stats.total, 6);
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.working, 3);
        assert_eq!(stats.done, 1);
        assert_eq!(stats.blocked, 1);
        assert_eq!(stats.error, 1);
    }
}
