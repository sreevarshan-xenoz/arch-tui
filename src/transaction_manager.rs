//! Orchestration of safe system modifications.
//!
//! Coordinates pre-operation snapshots, command execution, error
//! handling, and post-operation cleanup for package transactions.

use crate::errors::{AppError, Result};
use crate::hooks::HookRunner;
use crate::services::CommandSpec;
use crate::simulation::SimulationEngine;
use crate::traits::{PackageSimulator, SnapshotProvider};
use crate::watchdog::HealthWatchdog;
use std::sync::Arc;

/// Orchestrates safe package transactions with health checks, simulations, snapshots, and hooks.
pub struct TransactionManager {
    snapshot_provider: Option<Arc<dyn SnapshotProvider>>,
    simulator: Arc<SimulationEngine>,
    watchdog: Arc<HealthWatchdog>,
    keep_count: usize,
    hook_runner: Option<HookRunner>,
}

impl TransactionManager {
    /// Create a new TransactionManager
    pub fn new(
        snapshot_provider: Option<Arc<dyn SnapshotProvider>>,
        simulator: Arc<SimulationEngine>,
        watchdog: Arc<HealthWatchdog>,
        keep_count: usize,
    ) -> Self {
        Self {
            snapshot_provider,
            simulator,
            watchdog,
            keep_count,
            hook_runner: None,
        }
    }

    /// Set the hook runner from application configuration
    pub fn with_hooks(mut self, hook_runner: HookRunner) -> Self {
        self.hook_runner = Some(hook_runner);
        self
    }

    /// Get the snapshot provider if one is available
    pub fn get_snapshot_provider(&self) -> Option<Arc<dyn SnapshotProvider>> {
        self.snapshot_provider.clone()
    }

    /// Run pre-operation hooks and log results
    fn run_pre_hooks(&self, action_name: &str) {
        if let Some(ref runner) = self.hook_runner {
            let results = match action_name {
                name if name.contains("install") => runner.run_pre_install(),
                name if name.contains("remove") || name.contains("uninstall") => {
                    runner.run_pre_remove()
                }
                name if name.contains("update") || name.contains("upgrade") => {
                    runner.run_pre_update()
                }
                _ => Vec::new(),
            };

            for (i, result) in results.iter().enumerate() {
                match result {
                    Ok(output) => {
                        tracing::info!(
                            action = %action_name,
                            hook = i,
                            "Pre-hook succeeded: {}",
                            output.trim()
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            action = %action_name,
                            hook = i,
                            "Pre-hook failed: {}",
                            err
                        );
                    }
                }
            }
        }
    }

    /// Run post-operation hooks and log results
    fn run_post_hooks(&self, action_name: &str) {
        if let Some(ref runner) = self.hook_runner {
            let results = match action_name {
                name if name.contains("install") => runner.run_post_install(),
                name if name.contains("remove") || name.contains("uninstall") => {
                    runner.run_post_remove()
                }
                name if name.contains("update") || name.contains("upgrade") => {
                    runner.run_post_update()
                }
                _ => Vec::new(),
            };

            for (i, result) in results.iter().enumerate() {
                match result {
                    Ok(output) => {
                        tracing::info!(
                            action = %action_name,
                            hook = i,
                            "Post-hook succeeded: {}",
                            output.trim()
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            action = %action_name,
                            hook = i,
                            "Post-hook failed: {}",
                            err
                        );
                    }
                }
            }
        }
    }

    /// Execute an action within a safe transaction pipeline
    pub async fn run_safe_transaction<F, Fut, T>(
        &self,
        action_name: &str,
        commands: Option<&[CommandSpec]>,
        action: F,
    ) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        tracing::info!(action = %action_name, "Starting safe transaction");

        // 1. Health Check (Watchdog) - check for database lock
        if self.watchdog.check_db_lock().await? {
            tracing::error!("Package database is locked");
            return Err(AppError::Backend(
                "Package database is locked by another process".to_string(),
            ));
        }

        // 2. Simulation
        if let Some(cmds) = commands {
            tracing::info!("Running pre-transaction simulation...");
            // Extract package names from commands
            let mut packages = Vec::new();
            for cmd in cmds {
                let prog = cmd.prog.to_lowercase();
                if prog.contains("pacman") || prog == "sudo" || prog == "brew" || prog == "scoop" || prog == "apt-get" || prog == "apt" {
                    // Heuristic: anything that doesn't start with - is a package
                    // Skip keywords/flags
                    for arg in &cmd.args {
                        if !arg.starts_with('-')
                            && arg != "pacman"
                            && arg != "sudo"
                            && arg != "brew"
                            && arg != "scoop"
                            && arg != "apt-get"
                            && arg != "apt"
                            && arg != "install"
                            && arg != "remove"
                            && arg != "uninstall"
                            && arg != "-S"
                            && arg != "-R"
                            && arg != "-U"
                        {
                            packages.push(arg.as_str());
                        }
                    }
                }
            }

            if !packages.is_empty() {
                let sim_res = self.simulator.simulate_install(&packages).await?;
                if !sim_res.conflicts.is_empty() {
                    let conflicts = sim_res.conflicts.join(", ");
                    tracing::error!(conflicts = %conflicts, "Simulation detected conflicts, aborting");
                    return Err(AppError::Backend(format!(
                        "Transaction blocked by conflicts: {}",
                        conflicts
                    )));
                }
                tracing::info!("Simulation successful: no conflicts detected");
            }
        }

        // 3. Snapshot (if provider exists)
        let snapshot_id = if let Some(p) = &self.snapshot_provider {
            tracing::info!("Creating pre-transaction snapshot...");
            match p.create(action_name).await {
                Ok(id) => {
                    tracing::info!(snapshot_id = %id, "Snapshot created successfully");
                    Some(id)
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to create snapshot, aborting transaction");
                    return Err(e);
                }
            }
        } else {
            None
        };

        // 4. Run Pre-Hooks
        self.run_pre_hooks(action_name);

        // 5. Run Action
        let result = action().await;

        // 6. Handle success/failure
        match result {
            Ok(val) => {
                tracing::info!(action = %action_name, "Transaction completed successfully");

                // Run Post-Hooks
                self.run_post_hooks(action_name);

                // Cleanup old snapshots
                if let Some(p) = &self.snapshot_provider {
                    if let Err(e) = p.cleanup(self.keep_count).await {
                        tracing::warn!(error = %e, "Failed to cleanup old snapshots");
                    }
                }

                Ok(val)
            }
            Err(e) => {
                tracing::error!(
                    action = %action_name,
                    error = %e,
                    "CRITICAL: Transaction failed"
                );

                if let Some(id) = &snapshot_id {
                    tracing::warn!(
                        snapshot_id = %id,
                        "System may be in an inconsistent state. A snapshot is available for rollback."
                    );
                }

                Err(AppError::TransactionFailed(e.to_string(), snapshot_id))
            }
        }
    }
}
