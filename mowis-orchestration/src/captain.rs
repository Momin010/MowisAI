use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use crate::config::OrchConfig;
use crate::crew::{Crew, CrewOutcome, CrewTask};
use crate::events::{CaptainStatus, Event, EventBus};
use crate::plan::{Plan, PlanId, PlanStatus, TaskId, TaskNode};
use crate::tools::{crew_allowlist, ToolGateway};

#[derive(Debug)]
pub struct Captain {
    cfg:           Arc<OrchConfig>,
    bus:           EventBus,
    cmd_rx:        mpsc::Receiver<CaptainCommand>,
    cmd_tx:        mpsc::Sender<CaptainCommand>,
    plan:          Option<Plan>,
    sandbox_id:    Option<String>,
    completed:     HashMap<TaskId, CrewOutcome>,
    failed:        HashMap<TaskId, CrewOutcome>,
    injected_tasks: Vec<InjectedTask>,
}

#[derive(Debug)]
struct CrewHandle {
    agent_id: String,
    task_id: TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectedTask {
    pub id: String,
    pub title: String,
    pub description: String,
    pub deps: Vec<TaskId>,
    pub model_tier: String,
    pub tool_budget: u32,
    pub injected_at: String,
    pub reason: String,
}

#[derive(Debug)]
pub enum CaptainCommand {
    StartPlan {
        plan_id: PlanId,
        reply_tx: oneshot::Sender<Result<CaptainOutcome>>,
    },
    InjectTask {
        task: TaskNode,
        reply_tx: oneshot::Sender<Result<TaskId>>,
    },
    PauseAll {
        reply_tx: oneshot::Sender<()>,
    },
    ResumeAll {
        reply_tx: oneshot::Sender<()>,
    },
    CancelPlan {
        reason: String,
        reply_tx: oneshot::Sender<()>,
    },
    QueryStatus {
        reply_tx: oneshot::Sender<CaptainStatus>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CaptainOutcome {
    Completed { sandbox_id: String },
    Failed { reason: String, sandbox_id: String },
    Aborted,
}

impl Captain {
    pub fn new(
        cfg: &OrchConfig,
        bus: EventBus,
    ) -> (Self, mpsc::Sender<CaptainCommand>) {
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let captain = Self {
            cfg: Arc::new(cfg.clone()),
            bus,
            cmd_rx,
            cmd_tx: cmd_tx.clone(),
            plan: None,
            sandbox_id: None,
            completed: HashMap::new(),
            failed: HashMap::new(),
            injected_tasks: Vec::new(),
        };
        (captain, cmd_tx)
    }

    pub async fn run(mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        CaptainCommand::StartPlan { plan_id, reply_tx } => {
                            let result = self.start_plan(plan_id).await;
                            let _ = reply_tx.send(result);
                        }
                        CaptainCommand::InjectTask { task, reply_tx } => {
                            let result = self.inject_task(task).await;
                            let _ = reply_tx.send(result);
                        }
                        CaptainCommand::PauseAll { reply_tx } => {
                            self.pause_all().await;
                            let _ = reply_tx.send(());
                        }
                        CaptainCommand::ResumeAll { reply_tx } => {
                            self.resume_all().await;
                            let _ = reply_tx.send(());
                        }
                        CaptainCommand::CancelPlan { reason, reply_tx } => {
                            self.cancel_plan(&reason).await;
                            let _ = reply_tx.send(());
                        }
                        CaptainCommand::QueryStatus { reply_tx } => {
                            let status = self.query_status();
                            let _ = reply_tx.send(status);
                        }
                        CaptainCommand::Shutdown => {
                            self.shutdown().await;
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn start_plan(&mut self, plan_id: PlanId) -> Result<CaptainOutcome> {
        let plan = Plan::load(&self.cfg.plans_dir, &plan_id)?;

        let sandbox_id = if let Some(ref id) = self.sandbox_id {
            id.clone()
        } else {
            let id = format!("conv-{}", plan_id.0);
            self.sandbox_id = Some(id.clone());
            id
        };

        self.plan = Some(plan.clone());

        self.bus.emit(Event::CaptainStarted {
            plan_id: plan_id.clone(),
            sandbox_id: sandbox_id.clone(),
        });

        let task_graph = plan.task_graph();

        // Build dependency map
        let mut remaining: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
        let mut completed: std::collections::HashSet<TaskId> = std::collections::HashSet::new();
        let mut handles: HashMap<TaskId, tokio::task::JoinHandle<(TaskId, Result<CrewOutcome>)>> = HashMap::new();

        for task in &task_graph.tasks {
            remaining.insert(task.id.clone(), task.deps.clone());
        }

        // Event-driven scheduling: spawn tasks as soon as deps are met
        let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<(TaskId, Result<CrewOutcome>)>(64);
        let mut running = 0usize;

        loop {
            // Find all tasks whose deps are satisfied and not yet started
            let ready: Vec<TaskId> = remaining.iter()
                .filter(|(_, deps)| deps.iter().all(|d| completed.contains(d)))
                .map(|(id, _)| id.clone())
                .collect();

            if ready.is_empty() && running == 0 {
                break; // All done
            }

            // Spawn ready tasks
            for task_id in ready {
                remaining.remove(&task_id);
                let task_node = task_graph.tasks.iter()
                    .find(|t| t.id == task_id)
                    .ok_or_else(|| anyhow::anyhow!("task {} not found", task_id.0))?;

                let agent_id = format!("{}-{}", plan_id.0, task_id.0);
                let llm_config = self.cfg.llm_for_task(&plan, task_node)?;
                let tool_gateway = ToolGateway::new(
                    crate::plan::Tier::Crew,
                    crew_allowlist(),
                    sandbox_id.clone(),
                    agent_id.clone(),
                );
                let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                tool_gateway.set_transport(Box::new(crate::tools::LocalTransport::new(work_dir))).await;

                let crew_task = CrewTask {
                    task_id: task_node.id.clone(),
                    title: task_node.title.clone(),
                    description: task_node.description.clone(),
                    files_hint: task_node.files_hint.clone(),
                    tool_budget: task_node.tool_budget,
                };

                let crew = Crew::new(
                    plan_id.clone(),
                    sandbox_id.clone(),
                    agent_id.clone(),
                    crew_task,
                    llm_config,
                    tool_gateway,
                    self.bus.clone(),
                );

                let tx = done_tx.clone();
                tokio::spawn(async move {
                    let outcome = crew.run().await;
                    let _ = tx.send((task_id, outcome)).await;
                });
                running += 1;
            }

            // Wait for any one task to complete
            if running == 0 {
                break;
            }

            let (task_id, result) = done_rx.recv().await
                .ok_or_else(|| anyhow::anyhow!("channel closed"))?;
            running -= 1;

            match result {
                Ok(outcome) => {
                    match &outcome {
                        CrewOutcome::Done { agent_id, .. } => {
                            completed.insert(task_id.clone());
                            self.bus.emit(Event::MergeCompleted {
                                plan_id: plan_id.clone(),
                                agent_id: agent_id.clone(),
                            });
                            self.completed.insert(task_id, outcome);
                        }
                        CrewOutcome::Failed { agent_id, reason, .. } => {
                            self.bus.emit(Event::CrewFailed {
                                plan_id: plan_id.clone(),
                                agent_id: agent_id.clone(),
                                reason: reason.clone(),
                            });
                            self.failed.insert(task_id, outcome);
                        }
                    }
                }
                Err(e) => {
                    self.failed.insert(task_id, CrewOutcome::Failed {
                        agent_id: String::new(),
                        reason: e.to_string(),
                        tool_calls: 0,
                    });
                }
            }
        }

        // Also run injected tasks
        self.run_injected_tasks(&plan_id, &sandbox_id).await;

        if self.failed.is_empty() {
            self.bus.emit(Event::PlanCompleted {
                plan_id: plan_id.clone(),
            });
            Ok(CaptainOutcome::Completed { sandbox_id })
        } else {
            let reasons: Vec<String> = self
                .failed
                .values()
                .map(|f| match f {
                    CrewOutcome::Failed { reason, .. } => reason.clone(),
                    _ => String::new(),
                })
                .collect();
            let reason = reasons.join("; ");
            self.bus.emit(Event::PlanFailed {
                plan_id: plan_id.clone(),
                reason: reason.clone(),
            });
            Ok(CaptainOutcome::Failed {
                reason,
                sandbox_id,
            })
        }
    }

    async fn run_injected_tasks(&mut self, plan_id: &PlanId, sandbox_id: &str) {
        let injected: Vec<InjectedTask> = self.injected_tasks.drain(..).collect();
        for inj in injected {
            let task_node = TaskNode {
                id: TaskId(inj.id.clone()),
                title: inj.title.clone(),
                description: inj.description.clone(),
                deps: inj.deps.clone(),
                model_tier: crate::plan::ModelTier::Fast,
                tool_budget: inj.tool_budget,
                files_hint: vec![],
            };

            let agent_id = format!("{}-{}", plan_id.0, inj.id);
            let llm_config = match self.cfg.llm_for_task(
                self.plan.as_ref().unwrap(),
                &task_node,
            ) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error = %e, "failed to get llm config for injected task");
                    continue;
                }
            };

                let tool_gateway = ToolGateway::new(
                    crate::plan::Tier::Crew,
                    crew_allowlist(),
                    sandbox_id.to_string(),
                    agent_id.clone(),
                );
                let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                tool_gateway.set_transport(Box::new(crate::tools::LocalTransport::new(work_dir))).await;

            let crew_task = CrewTask {
                task_id: task_node.id.clone(),
                title: task_node.title.clone(),
                description: task_node.description.clone(),
                files_hint: task_node.files_hint.clone(),
                tool_budget: task_node.tool_budget,
            };

            let crew = Crew::new(
                plan_id.clone(),
                sandbox_id.to_string(),
                agent_id.clone(),
                crew_task,
                llm_config,
                tool_gateway,
                self.bus.clone(),
            );

            match crew.run().await {
                Ok(outcome) => {
                    self.bus.emit(Event::MergeCompleted {
                        plan_id: plan_id.clone(),
                        agent_id: agent_id.clone(),
                    });
                    self.completed.insert(TaskId(inj.id.clone()), outcome);
                }
                Err(e) => {
                    tracing::error!(error = %e, task = %inj.id, "injected crew failed");
                }
            }
        }
    }

    async fn inject_task(&mut self, task: TaskNode) -> Result<TaskId> {
        let task_id = task.id.clone();
        let inj = InjectedTask {
            id: task_id.0.clone(),
            title: task.title.clone(),
            description: task.description.clone(),
            deps: task.deps.clone(),
            model_tier: format!("{:?}", task.model_tier).to_lowercase(),
            tool_budget: task.tool_budget,
            injected_at: chrono::Utc::now().to_rfc3339(),
            reason: "user mid-run inject".into(),
        };
        self.injected_tasks.push(inj);

        self.bus.emit(Event::TaskInjected {
            plan_id: self
                .plan
                .as_ref()
                .map(|p| p.plan_id.clone())
                .unwrap_or_else(|| PlanId("unknown".into())),
            task_id: task_id.clone(),
            reason: "user mid-run inject".into(),
        });

        Ok(task_id)
    }

    async fn pause_all(&self) {
        tracing::info!("pausing all in-flight crews");
        // In full impl, would send pause signals to each crew
    }

    async fn resume_all(&self) {
        tracing::info!("resuming all paused crews");
    }

    async fn cancel_plan(&mut self, reason: &str) {
        tracing::info!(reason = %reason, "cancelling plan");
        if let Some(ref plan) = self.plan {
            self.bus.emit(Event::PlanFailed {
                plan_id: plan.plan_id.clone(),
                reason: reason.to_string(),
            });
        }
    }

    fn query_status(&self) -> CaptainStatus {
        CaptainStatus {
            plan_id: self.plan.as_ref().map(|p| p.plan_id.clone()),
            sandbox_id: self.sandbox_id.clone(),
            in_flight: vec![], // Tasks are tracked in completed/failed
            completed: self.completed.keys().cloned().collect(),
            failed: self
                .failed
                .iter()
                .map(|(id, outcome)| {
                    let reason = match outcome {
                        CrewOutcome::Failed { reason, .. } => reason.clone(),
                        _ => String::new(),
                    };
                    (id.clone(), reason)
                })
                .collect(),
        }
    }

    async fn shutdown(&mut self) {
        tracing::info!("captain shutting down");
        if let Some(ref sandbox_id) = self.sandbox_id {
            let status = if self.failed.is_empty() {
                PlanStatus::Done
            } else {
                PlanStatus::Aborted
            };
            self.bus.emit(Event::CaptainShutdown {
                sandbox_id: sandbox_id.clone(),
                final_plan_status: status,
            });
        }
    }
}

/// Simple synchronous constructor for backward compatibility with mowis-host chat
impl Captain {
    pub fn new_simple(cfg: &OrchConfig, plan_id: PlanId, bus: EventBus) -> Result<SimpleCaptain> {
        Ok(SimpleCaptain {
            cfg: cfg.clone(),
            plan_id,
            bus,
        })
    }
}

pub struct SimpleCaptain {
    cfg: OrchConfig,
    plan_id: PlanId,
    bus: EventBus,
}

impl SimpleCaptain {
    pub fn new(cfg: &OrchConfig, plan_id: PlanId, bus: EventBus) -> Result<Self> {
        Ok(Self {
            cfg: cfg.clone(),
            plan_id,
            bus,
        })
    }

    pub async fn run(self) -> Result<CaptainOutcome> {
        let plan = Plan::load(&self.cfg.plans_dir, &self.plan_id)?;
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
                let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                tool_gateway.set_transport(Box::new(crate::tools::LocalTransport::new(work_dir))).await;

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

                let task_id_owned = task_id.clone();
                handles.push(tokio::spawn(async move {
                    let outcome = crew.run().await;
                    (task_id_owned, outcome)
                }));
            }

            for handle in handles {
                match handle.await {
                    Ok((task_id, Ok(outcome))) => match &outcome {
                        CrewOutcome::Done { agent_id, .. } => {
                            self.bus.emit(Event::MergeCompleted {
                                plan_id: self.plan_id.clone(),
                                agent_id: agent_id.clone(),
                            });
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

fn topological_sort(tasks: &[TaskNode]) -> Result<Vec<Vec<TaskId>>> {
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
