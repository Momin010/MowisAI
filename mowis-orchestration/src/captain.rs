use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::config::OrchConfig;
use crate::crew::{Crew, CrewOutcome, CrewTask};
use crate::events::{Event, EventBus};
use crate::plan::{Plan, PlanId, TaskId};
use crate::tools::{crew_allowlist, ToolGateway};

#[derive(Debug, Clone)]
pub struct Captain {
    cfg: OrchConfig,
    plan_id: PlanId,
    bus: EventBus,
}

impl Captain {
    pub fn new(cfg: &OrchConfig, plan_id: PlanId, bus: EventBus) -> Result<Self> {
        Ok(Self {
            cfg: cfg.clone(),
            plan_id,
            bus,
        })
    }

    pub async fn run(mut self) -> Result<CaptainOutcome> {
        let plan_dir = self.cfg.plans_dir.clone();
        let plan = Plan::load(&plan_dir, &self.plan_id)?;

        let sandbox_id = format!("conv-{}", self.plan_id.0);

        self.bus.emit(Event::CaptainStarted {
            plan_id: self.plan_id.clone(),
            sandbox_id: sandbox_id.clone(),
        });

        let task_graph = plan.task_graph();
        let execution_order = topological_sort(&task_graph.tasks)?;

        let mut completed: HashMap<TaskId, CrewOutcome> = HashMap::new();
        let mut failed: HashMap<TaskId, CrewOutcome> = HashMap::new();

        for wave in execution_order {
            let mut handles = Vec::new();

            for task_id in &wave {
                let task_node = task_graph
                    .tasks
                    .iter()
                    .find(|t| &t.id == task_id)
                    .ok_or_else(|| anyhow::anyhow!("task {} not found", task_id.0))?;

                let agent_id = format!("{}-{}", self.plan_id.0, task_id.0);

                let llm_config = self.cfg.llm_for_task(&plan, task_node)?;

                let tool_gateway = ToolGateway::new(
                    crate::plan::Tier::Crew,
                    crew_allowlist(),
                    sandbox_id.clone(),
                    agent_id.clone(),
                );

                let crew_task = CrewTask {
                    task_id: task_node.id.clone(),
                    title: task_node.title.clone(),
                    description: task_node.description.clone(),
                    files_hint: task_node.files_hint.clone(),
                    tool_budget: task_node.tool_budget,
                };

                let crew = Crew::new(
                    self.plan_id.clone(),
                    sandbox_id.clone(),
                    agent_id.clone(),
                    crew_task,
                    llm_config,
                    tool_gateway,
                    self.bus.clone(),
                );

                let bus = self.bus.clone();
                let plan_id = self.plan_id.clone();
                let task_id_clone = task_id.clone();

                handles.push(tokio::spawn(async move {
                    let outcome = crew.run().await;
                    (task_id_clone, outcome)
                }));
            }

            for handle in handles {
                match handle.await {
                    Ok((task_id, Ok(outcome))) => match &outcome {
                        CrewOutcome::Done { .. } => {
                            completed.insert(task_id, outcome);
                        }
                        CrewOutcome::Failed { .. } => {
                            failed.insert(task_id, outcome);
                        }
                    },
                    Ok((task_id, Err(e))) => {
                        failed.insert(
                            task_id,
                            CrewOutcome::Failed {
                                agent_id: String::new(),
                                reason: e.to_string(),
                                tool_calls: 0,
                            },
                        );
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "crew task panicked");
                    }
                }
            }
        }

        if failed.is_empty() {
            self.bus.emit(Event::PlanCompleted {
                plan_id: self.plan_id.clone(),
            });
            Ok(CaptainOutcome::Completed { sandbox_id })
        } else {
            let reasons: Vec<String> = failed
                .values()
                .map(|f| match f {
                    CrewOutcome::Failed { reason, .. } => reason.clone(),
                    _ => String::new(),
                })
                .collect();
            let reason = reasons.join("; ");
            self.bus.emit(Event::PlanFailed {
                plan_id: self.plan_id.clone(),
                reason: reason.clone(),
            });
            Ok(CaptainOutcome::Failed {
                reason,
                sandbox_id,
            })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CaptainOutcome {
    Completed { sandbox_id: String },
    Failed { reason: String, sandbox_id: String },
    Aborted,
}

fn topological_sort(tasks: &[crate::plan::TaskNode]) -> Result<Vec<Vec<TaskId>>> {
    let mut in_degree: HashMap<TaskId, usize> = HashMap::new();
    let mut dependents: HashMap<TaskId, Vec<TaskId>> = HashMap::new();

    for task in tasks {
        in_degree.entry(task.id.clone()).or_insert(0);
        for dep in &task.deps {
            dependents.entry(dep.clone()).or_default().push(task.id.clone());
            *in_degree.entry(task.id.clone()).or_insert(0) += 1;
        }
    }

    let mut waves = Vec::new();
    let mut remaining: std::collections::HashSet<TaskId> =
        tasks.iter().map(|t| t.id.clone()).collect();

    while !remaining.is_empty() {
        let wave: Vec<TaskId> = remaining
            .iter()
            .filter(|id| in_degree.get(id).copied().unwrap_or(0) == 0)
            .cloned()
            .collect();

        if wave.is_empty() {
            anyhow::bail!("cycle detected in task graph");
        }

        for task_id in &wave {
            remaining.remove(task_id);
            if let Some(deps) = dependents.get(task_id) {
                for dep in deps {
                    if let Some(degree) = in_degree.get_mut(dep) {
                        *degree = degree.saturating_sub(1);
                    }
                }
            }
        }

        waves.push(wave);
    }

    Ok(waves)
}
