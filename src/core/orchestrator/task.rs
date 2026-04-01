use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Blocked,
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: String,
    pub assignee: Option<String>,
    pub depends_on: Vec<String>,
    pub status: TaskStatus,
    pub result: Option<JsonValue>,
    pub error: Option<String>,
    /// Optional backend override for multi-model workflows.
    pub backend: Option<String>,
}

impl Task {
    pub fn new(id: &str, title: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            assignee: None,
            depends_on: Vec::new(),
            status: TaskStatus::Pending,
            result: None,
            error: None,
            backend: None,
        }
    }
}

pub struct TaskQueue {
    tasks: HashMap<String, Task>,
    insertion_order: Vec<String>,
}

impl TaskQueue {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            insertion_order: Vec::new(),
        }
    }

    /// Add a task. Resolves initial status based on dependencies.
    pub fn add(&mut self, mut task: Task) -> anyhow::Result<()> {
        // Validate no self-dependency
        if task.depends_on.contains(&task.id) {
            anyhow::bail!("Task '{}' depends on itself", task.id);
        }

        // Check all dependencies exist or are being added
        for dep in &task.depends_on {
            if dep == &task.id {
                anyhow::bail!("Task '{}' depends on itself", task.id);
            }
        }

        // Set initial status
        if task.depends_on.is_empty() {
            task.status = TaskStatus::Pending;
        } else {
            let all_deps_met = task.depends_on.iter().all(|dep_id| {
                self.tasks
                    .get(dep_id)
                    .map(|t| t.status == TaskStatus::Completed)
                    .unwrap_or(false)
            });
            task.status = if all_deps_met {
                TaskStatus::Pending
            } else {
                TaskStatus::Blocked
            };
        }

        self.insertion_order.push(task.id.clone());
        self.tasks.insert(task.id.clone(), task);

        // Run full cycle detection after insertion
        if let Err(e) = self.check_cycles() {
            // Roll back: remove the task we just added
            let id = self.insertion_order.pop().unwrap();
            self.tasks.remove(&id);
            return Err(e);
        }

        Ok(())
    }

    /// Load tasks from a list, resolving title-based dependencies to IDs.
    pub fn load_tasks(&mut self, tasks: Vec<Task>) -> anyhow::Result<()> {
        // Build title→id map
        let title_to_id: HashMap<String, String> = tasks
            .iter()
            .map(|t| (t.title.clone(), t.id.clone()))
            .collect();

        for mut task in tasks {
            // Resolve title-based deps to IDs
            task.depends_on = task
                .depends_on
                .iter()
                .map(|dep| {
                    title_to_id
                        .get(dep)
                        .cloned()
                        .unwrap_or_else(|| dep.clone())
                })
                .collect();

            self.add(task)?;
        }

        // Detect cycles
        self.check_cycles()?;

        Ok(())
    }

    /// Get all tasks that are ready to run.
    pub fn next_ready(&self) -> Vec<&Task> {
        self.insertion_order
            .iter()
            .filter_map(|id| self.tasks.get(id))
            .filter(|t| t.status == TaskStatus::Pending)
            .collect()
    }

    /// Check if any tasks are in progress.
    pub fn has_in_progress(&self) -> bool {
        self.tasks.values().any(|t| t.status == TaskStatus::InProgress)
    }

    /// Check if all tasks are terminal (completed or failed).
    pub fn is_done(&self) -> bool {
        self.tasks.values().all(|t| {
            t.status == TaskStatus::Completed || t.status == TaskStatus::Failed
        })
    }

    /// Mark a task as in-progress.
    pub fn start(&mut self, task_id: &str) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskStatus::InProgress;
        }
    }

    /// Mark a task as completed and unblock dependents.
    pub fn complete(&mut self, task_id: &str, result: JsonValue) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskStatus::Completed;
            task.result = Some(result);
        }
        self.promote_blocked();
    }

    /// Mark a task as failed and cascade failure to dependents.
    pub fn fail(&mut self, task_id: &str, error: &str) {
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.status = TaskStatus::Failed;
            task.error = Some(error.to_string());
        }
        self.cascade_failures(task_id);
    }

    /// Get a task by ID.
    pub fn get(&self, task_id: &str) -> Option<&Task> {
        self.tasks.get(task_id)
    }

    /// Get all tasks in insertion order.
    pub fn all_tasks(&self) -> Vec<&Task> {
        self.insertion_order
            .iter()
            .filter_map(|id| self.tasks.get(id))
            .collect()
    }

    /// Promote blocked tasks whose dependencies are all completed.
    fn promote_blocked(&mut self) {
        let completed: HashSet<String> = self
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Completed)
            .map(|t| t.id.clone())
            .collect();

        for task in self.tasks.values_mut() {
            if task.status == TaskStatus::Blocked {
                let all_met = task.depends_on.iter().all(|dep| completed.contains(dep));
                if all_met {
                    task.status = TaskStatus::Pending;
                }
            }
        }
    }

    /// Cascade failure: mark all transitive dependents as failed.
    fn cascade_failures(&mut self, failed_id: &str) {
        let mut to_fail: Vec<String> = Vec::new();
        let mut frontier = vec![failed_id.to_string()];

        while let Some(id) = frontier.pop() {
            for task in self.tasks.values() {
                if task.depends_on.contains(&id)
                    && task.status != TaskStatus::Failed
                    && task.status != TaskStatus::Completed
                {
                    to_fail.push(task.id.clone());
                    frontier.push(task.id.clone());
                }
            }
        }

        for id in to_fail {
            if let Some(task) = self.tasks.get_mut(&id) {
                task.status = TaskStatus::Failed;
                task.error = Some(format!(
                    "Dependency '{}' failed",
                    failed_id
                ));
            }
        }
    }

    /// Check for dependency cycles using DFS.
    fn check_cycles(&self) -> anyhow::Result<()> {
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();

        for id in self.tasks.keys() {
            if !visited.contains(id) {
                self.dfs_cycle(id, &mut visited, &mut in_stack)?;
            }
        }
        Ok(())
    }

    fn dfs_cycle(
        &self,
        id: &str,
        visited: &mut HashSet<String>,
        in_stack: &mut HashSet<String>,
    ) -> anyhow::Result<()> {
        visited.insert(id.to_string());
        in_stack.insert(id.to_string());

        if let Some(task) = self.tasks.get(id) {
            for dep in &task.depends_on {
                if !visited.contains(dep) {
                    self.dfs_cycle(dep, visited, in_stack)?;
                } else if in_stack.contains(dep) {
                    anyhow::bail!("Dependency cycle detected involving task '{}'", dep);
                }
            }
        }

        in_stack.remove(id);
        Ok(())
    }

    /// Count how many downstream tasks depend (transitively) on a given task.
    /// Used by the scheduler for critical-path prioritization.
    pub fn downstream_count(&self, task_id: &str) -> usize {
        let mut count = 0;
        let mut frontier = vec![task_id.to_string()];
        let mut seen = HashSet::new();
        seen.insert(task_id.to_string());

        while let Some(id) = frontier.pop() {
            for task in self.tasks.values() {
                if task.depends_on.contains(&id) && seen.insert(task.id.clone()) {
                    count += 1;
                    frontier.push(task.id.clone());
                }
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_without_deps_starts_pending() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "Task 1", "Do something")).unwrap();
        assert_eq!(q.get("t1").unwrap().status, TaskStatus::Pending);
    }

    #[test]
    fn task_with_unmet_deps_starts_blocked() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "Task 1", "Do something")).unwrap();
        let mut t2 = Task::new("t2", "Task 2", "Do more");
        t2.depends_on = vec!["t1".to_string()];
        q.add(t2).unwrap();
        assert_eq!(q.get("t2").unwrap().status, TaskStatus::Blocked);
    }

    #[test]
    fn completing_dep_promotes_blocked() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "Task 1", "Do something")).unwrap();
        let mut t2 = Task::new("t2", "Task 2", "Do more");
        t2.depends_on = vec!["t1".to_string()];
        q.add(t2).unwrap();

        q.complete("t1", json!("done"));
        assert_eq!(q.get("t2").unwrap().status, TaskStatus::Pending);
    }

    #[test]
    fn failure_cascades_to_dependents() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "A", "")).unwrap();
        let mut t2 = Task::new("t2", "B", "");
        t2.depends_on = vec!["t1".to_string()];
        q.add(t2).unwrap();
        let mut t3 = Task::new("t3", "C", "");
        t3.depends_on = vec!["t2".to_string()];
        q.add(t3).unwrap();

        q.fail("t1", "boom");
        assert_eq!(q.get("t2").unwrap().status, TaskStatus::Failed);
        assert_eq!(q.get("t3").unwrap().status, TaskStatus::Failed);
    }

    #[test]
    fn cycle_detection() {
        let mut q = TaskQueue::new();
        let mut t1 = Task::new("t1", "A", "");
        t1.depends_on = vec!["t2".to_string()];
        q.tasks.insert("t1".to_string(), t1);
        let mut t2 = Task::new("t2", "B", "");
        t2.depends_on = vec!["t1".to_string()];
        q.tasks.insert("t2".to_string(), t2);

        let err = q.check_cycles().expect_err("should detect cycle");
        assert!(err.to_string().contains("cycle"));
    }

    #[test]
    fn parallel_tasks_both_ready() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "A", "")).unwrap();
        q.add(Task::new("t2", "B", "")).unwrap();
        assert_eq!(q.next_ready().len(), 2);
    }

    #[test]
    fn downstream_count_works() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "A", "")).unwrap();
        let mut t2 = Task::new("t2", "B", "");
        t2.depends_on = vec!["t1".to_string()];
        q.add(t2).unwrap();
        let mut t3 = Task::new("t3", "C", "");
        t3.depends_on = vec!["t1".to_string()];
        q.add(t3).unwrap();

        assert_eq!(q.downstream_count("t1"), 2);
        assert_eq!(q.downstream_count("t2"), 0);
    }
}
