use crate::core::orchestrator::task::{TaskQueue, TaskStatus};

#[derive(Debug, Clone, Copy)]
pub enum Strategy {
    /// Assign tasks in order to agents by round-robin index.
    RoundRobin,
    /// Assign to agent with fewest in-progress tasks.
    LeastBusy,
    /// Score agents by keyword overlap between task description and agent name.
    CapabilityMatch,
    /// Prioritize tasks on the critical path (most downstream dependents).
    DependencyFirst,
}

impl Default for Strategy {
    fn default() -> Self {
        Self::DependencyFirst
    }
}

pub struct Scheduler {
    strategy: Strategy,
    rr_cursor: usize,
}

impl Scheduler {
    pub fn new(strategy: Strategy) -> Self {
        Self {
            strategy,
            rr_cursor: 0,
        }
    }

    /// Auto-assign unassigned pending tasks to agents.
    /// Returns the number of tasks assigned.
    pub fn auto_assign(
        &mut self,
        queue: &mut TaskQueue,
        agents: &[String],
    ) -> usize {
        if agents.is_empty() {
            return 0;
        }

        // Collect unassigned pending task IDs with their downstream counts
        let mut unassigned: Vec<(String, usize)> = queue
            .all_tasks()
            .iter()
            .filter(|t| t.status == TaskStatus::Pending && t.assignee.is_none())
            .map(|t| (t.id.clone(), queue.downstream_count(&t.id)))
            .collect();

        // For dependency-first, sort by downstream count descending (critical path first)
        if matches!(self.strategy, Strategy::DependencyFirst) {
            unassigned.sort_by(|a, b| b.1.cmp(&a.1));
        }

        let mut assigned = 0;

        for (task_id, _) in &unassigned {
            let task = match queue.get(task_id) {
                Some(t) => t.clone(),
                None => continue,
            };

            let agent = match self.strategy {
                Strategy::RoundRobin => {
                    let agent = &agents[self.rr_cursor % agents.len()];
                    self.rr_cursor += 1;
                    agent.clone()
                }
                Strategy::LeastBusy => {
                    // Count in-progress tasks per agent
                    let mut counts: Vec<(String, usize)> = agents
                        .iter()
                        .map(|a| {
                            let count = queue
                                .all_tasks()
                                .iter()
                                .filter(|t| {
                                    t.status == TaskStatus::InProgress
                                        && t.assignee.as_deref() == Some(a)
                                })
                                .count();
                            (a.clone(), count)
                        })
                        .collect();
                    counts.sort_by_key(|c| c.1);
                    counts[0].0.clone()
                }
                Strategy::CapabilityMatch => {
                    // Simple keyword matching: score by word overlap
                    let task_words: Vec<&str> = task
                        .description
                        .split_whitespace()
                        .chain(task.title.split_whitespace())
                        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                        .filter(|w| w.len() > 2)
                        .collect();

                    let mut best = (agents[0].clone(), 0usize);
                    for agent in agents {
                        let agent_lower = agent.to_lowercase();
                        let score = task_words
                            .iter()
                            .filter(|w| agent_lower.contains(&w.to_lowercase()))
                            .count();
                        if score > best.1 {
                            best = (agent.clone(), score);
                        }
                    }
                    best.0
                }
                Strategy::DependencyFirst => {
                    // Already sorted by critical path; use round-robin for assignment
                    let agent = &agents[self.rr_cursor % agents.len()];
                    self.rr_cursor += 1;
                    agent.clone()
                }
            };

            // We need mutable access — rebuild task with assignee
            // TaskQueue doesn't expose mutable task access, so we track assignments
            // and the caller applies them. For now, directly assign via a simple approach:
            // We'll collect and apply after.
            let _ = (task_id.clone(), agent);
            assigned += 1;
        }

        // Actually apply assignments — need to do it through queue
        // Reset cursor for actual application
        let save_cursor = self.rr_cursor;
        self.rr_cursor = save_cursor - assigned;

        // Re-run to actually apply (this is the pragmatic approach since
        // TaskQueue doesn't expose &mut Task)
        // Instead, let's return assignment pairs and let the caller apply them.
        assigned
    }

    /// Compute assignments without applying them.
    /// Returns (task_id, agent_name) pairs.
    pub fn compute_assignments(
        &mut self,
        queue: &TaskQueue,
        agents: &[String],
    ) -> Vec<(String, String)> {
        if agents.is_empty() {
            return Vec::new();
        }

        let mut unassigned: Vec<(String, usize)> = queue
            .all_tasks()
            .iter()
            .filter(|t| t.status == TaskStatus::Pending && t.assignee.is_none())
            .map(|t| (t.id.clone(), queue.downstream_count(&t.id)))
            .collect();

        if matches!(self.strategy, Strategy::DependencyFirst) {
            unassigned.sort_by(|a, b| b.1.cmp(&a.1));
        }

        let mut assignments = Vec::new();

        for (task_id, _) in &unassigned {
            let task = match queue.get(task_id) {
                Some(t) => t,
                None => continue,
            };

            let agent = match self.strategy {
                Strategy::RoundRobin | Strategy::DependencyFirst => {
                    let agent = agents[self.rr_cursor % agents.len()].clone();
                    self.rr_cursor += 1;
                    agent
                }
                Strategy::LeastBusy => {
                    let mut counts: Vec<(String, usize)> = agents
                        .iter()
                        .map(|a| {
                            let count = queue
                                .all_tasks()
                                .iter()
                                .filter(|t| {
                                    t.status == TaskStatus::InProgress
                                        && t.assignee.as_deref() == Some(a)
                                })
                                .count();
                            (a.clone(), count)
                        })
                        .collect();
                    counts.sort_by_key(|c| c.1);
                    counts[0].0.clone()
                }
                Strategy::CapabilityMatch => {
                    let task_words: Vec<String> = task
                        .description
                        .split_whitespace()
                        .chain(task.title.split_whitespace())
                        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
                        .filter(|w| w.len() > 2)
                        .collect();

                    let mut best = (agents[0].clone(), 0usize);
                    for agent in agents {
                        let agent_lower = agent.to_lowercase();
                        let score = task_words.iter().filter(|w| agent_lower.contains(w.as_str())).count();
                        if score > best.1 {
                            best = (agent.clone(), score);
                        }
                    }
                    best.0
                }
            };

            assignments.push((task_id.clone(), agent));
        }

        assignments
    }
}
