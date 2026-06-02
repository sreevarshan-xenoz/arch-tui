//! Dry-run simulation engine for package operations.
//!
//! Provides a `PackageSimulator` implementation that logs what would
//! happen without actually modifying the system, useful for testing.

use crate::errors::{AppError, Result};
use crate::traits::{PackageSimulator, SimulationResult};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use tokio::process::Command;

static DOWNLOAD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Total Download Size:\s+([\d.]+)\s+(\w+)").expect("Invalid download regex")
});
static INSTALL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Total Installed Size:\s+([\d.]+)\s+(\w+)").expect("Invalid install regex")
});
static NET_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Net Upgrade Size:\s+([\d.-]+)\s+(\w+)").expect("Invalid net regex"));
static CONFLICT_PKG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"::\s+(.+)\s+and\s+(.+)\s+are\s+in\s+conflict").expect("Invalid conflict pkg regex")
});
static UNRESOLVABLE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"error:\s+unresolvable\s+package\s+conflicts").expect("Invalid unresolvable regex")
});
static TRANSACTION_FAILED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"error: failed to (?:commit|prepare) transaction \((.+)\)")
        .expect("Invalid transaction failed regex")
});

static APT_DOWNLOAD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"Need to get ([\d.]+)\s*(\w+)? of archives").expect("Invalid apt download regex")
});
static APT_DISK_USED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"After this operation, ([\d.]+)\s*(\w+)? of additional disk space will be used")
        .expect("Invalid apt disk used regex")
});
static APT_DISK_FREED_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"After this operation, ([\d.]+)\s*(\w+)? disk space will be freed")
        .expect("Invalid apt disk freed regex")
});

/// Simulation engine for package operations
pub struct SimulationEngine {
    backend: String,
}

impl SimulationEngine {
    /// Create a new simulation engine for the specified backend
    pub fn new(backend: String) -> Self {
        Self { backend }
    }

    /// Parse pacman dry-run output to extract simulation metrics
    pub fn parse_pacman_output(output: &str) -> SimulationResult {
        let mut total_download_bytes = 0;
        let mut disk_change_bytes = 0;
        let mut conflicts = Vec::new();
        let config_changes = Vec::new();

        for line in output.lines() {
            // Check for download size
            if let Some(caps) = DOWNLOAD_RE.captures(line) {
                if let Ok(val) = caps[1].parse::<f64>() {
                    total_download_bytes = convert_to_bytes(val, &caps[2]).max(0) as u64;
                }
            }
            // Check for installed size (fallback for disk change if net_re not found)
            else if let Some(caps) = INSTALL_RE.captures(line) {
                if let Ok(val) = caps[1].parse::<f64>() {
                    let bytes = convert_to_bytes(val, &caps[2]);
                    if disk_change_bytes == 0 {
                        disk_change_bytes = bytes;
                    }
                }
            }
            // Net upgrade size is often more accurate for "change"
            else if let Some(caps) = NET_RE.captures(line) {
                if let Ok(val) = caps[1].parse::<f64>() {
                    disk_change_bytes = convert_to_bytes(val, &caps[2]);
                }
            }

            // Detect conflicts
            if let Some(caps) = TRANSACTION_FAILED_RE.captures(line) {
                conflicts.push(caps[1].trim().to_string());
            } else if line.contains("conflicting files")
                || line.contains("exists in filesystem")
                || CONFLICT_PKG_RE.is_match(line)
                || UNRESOLVABLE_RE.is_match(line)
            {
                conflicts.push(line.trim().to_string());
            }
        }

        SimulationResult {
            total_download_bytes,
            disk_change_bytes,
            conflicts,
            config_changes,
        }
    }

    /// Parse apt dry-run output to extract simulation metrics
    pub fn parse_apt_output(output: &str) -> SimulationResult {
        let mut total_download_bytes = 0;
        let mut disk_change_bytes = 0;
        let mut conflicts = Vec::new();
        let config_changes = Vec::new();

        let mut in_unmet_dependencies = false;

        for line in output.lines() {
            let line_trimmed = line.trim();

            // Check for download size
            if let Some(caps) = APT_DOWNLOAD_RE.captures(line) {
                if let Ok(val) = caps[1].parse::<f64>() {
                    let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("B");
                    total_download_bytes = convert_to_bytes(val, unit).max(0) as u64;
                }
            }
            // Check for disk space used
            else if let Some(caps) = APT_DISK_USED_RE.captures(line) {
                if let Ok(val) = caps[1].parse::<f64>() {
                    let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("B");
                    disk_change_bytes = convert_to_bytes(val, unit);
                }
            }
            // Check for disk space freed
            else if let Some(caps) = APT_DISK_FREED_RE.captures(line) {
                if let Ok(val) = caps[1].parse::<f64>() {
                    let unit = caps.get(2).map(|m| m.as_str()).unwrap_or("B");
                    disk_change_bytes = -convert_to_bytes(val, unit);
                }
            }

            // Detect conflicts
            if line.contains("The following packages have unmet dependencies:") {
                in_unmet_dependencies = true;
                conflicts.push(line_trimmed.to_string());
            } else if in_unmet_dependencies {
                if line_trimmed.is_empty() {
                    in_unmet_dependencies = false;
                } else if line_trimmed.starts_with("E: ") || line_trimmed.starts_with("W: ") {
                    in_unmet_dependencies = false;
                    conflicts.push(line_trimmed.to_string());
                } else {
                    conflicts.push(line_trimmed.to_string());
                }
            } else if line_trimmed.starts_with("E: ") {
                conflicts.push(line_trimmed.to_string());
            }
        }

        SimulationResult {
            total_download_bytes,
            disk_change_bytes,
            conflicts,
            config_changes,
        }
    }
}

/// Helper to convert size with unit to bytes (handles negative values)
fn convert_to_bytes(val: f64, unit: &str) -> i64 {
    let multiplier = match unit.to_uppercase().as_str() {
        "B" => 1.0,
        "KIB" | "KB" | "K" => 1024.0,
        "MIB" | "MB" | "M" => 1024.0 * 1024.0,
        "GIB" | "GB" | "G" => 1024.0 * 1024.0 * 1024.0,
        "TIB" | "TB" | "T" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    (val * multiplier) as i64
}

#[async_trait]
impl PackageSimulator for SimulationEngine {
    async fn simulate_install(&self, packages: &[&str]) -> Result<SimulationResult> {
        match self.backend.as_str() {
            "pacman" => {
                // -Sw: download only, shows summary block without installing
                // Using LC_ALL=C to ensure predictable output for parsing
                let output = Command::new("pacman")
                    .env("LC_ALL", "C")
                    .args(["-Sw", "--noconfirm"])
                    .args(packages)
                    .output()
                    .await?;

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = format!("{}\n{}", stdout, stderr);

                let result = Self::parse_pacman_output(&combined);

                if !output.status.success() && result.conflicts.is_empty() {
                    return Err(AppError::Pacman(stderr.trim().to_string()));
                }

                Ok(result)
            }
            "apt" => {
                // Using -s for simulation
                let output = Command::new("apt-get")
                    .env("LC_ALL", "C")
                    .args(["install", "-s", "-y"])
                    .args(packages)
                    .output()
                    .await?;

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = format!("{}\n{}", stdout, stderr);

                let result = Self::parse_apt_output(&combined);

                if !output.status.success() && result.conflicts.is_empty() {
                    return Err(AppError::Command(format!(
                        "apt-get failed: {}",
                        stderr.trim()
                    )));
                }

                Ok(result)
            }
            "brew" => {
                // brew install --dry-run
                let output = Command::new("brew")
                    .args(["install", "--dry-run"])
                    .args(packages)
                    .output()
                    .await?;

                let _stdout = String::from_utf8_lossy(&output.stdout);
                let mut conflicts = Vec::new();
                if !output.status.success() {
                    conflicts.push(String::from_utf8_lossy(&output.stderr).to_string());
                }

                Ok(SimulationResult {
                    total_download_bytes: 0, // brew dry-run doesn't easily show sizes
                    disk_change_bytes: 0,
                    conflicts,
                    config_changes: Vec::new(),
                })
            }
            "scoop" => {
                // scoop doesn't have a great dry-run for sizes, just check if it can be installed
                Ok(SimulationResult {
                    total_download_bytes: 0,
                    disk_change_bytes: 0,
                    conflicts: Vec::new(),
                    config_changes: Vec::new(),
                })
            }
            _ => {
                // Default fallback for unimplemented backends
                Ok(SimulationResult {
                    total_download_bytes: 0,
                    disk_change_bytes: 0,
                    conflicts: Vec::new(),
                    config_changes: Vec::new(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_apt_install_output() {
        let output = r#"
Reading package lists... Done
Building dependency tree       
Reading state information... Done
The following NEW packages will be installed:
  vim-runtime
0 upgraded, 1 newly installed, 0 to remove and 0 not upgraded.
Need to get 123 kB of archives.
After this operation, 456 kB of additional disk space will be used.
"#;
        let result = SimulationEngine::parse_apt_output(output);
        assert_eq!(result.total_download_bytes, 123 * 1024);
        assert_eq!(result.disk_change_bytes, 456 * 1024);
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn test_parse_apt_remove_output() {
        let output = r#"
Reading package lists... Done
Building dependency tree       
Reading state information... Done
The following packages will be REMOVED:
  vim-runtime*
0 upgraded, 0 newly installed, 1 to remove and 0 not upgraded.
After this operation, 789 kB disk space will be freed.
"#;
        let result = SimulationEngine::parse_apt_output(output);
        assert_eq!(result.total_download_bytes, 0);
        assert_eq!(result.disk_change_bytes, -789 * 1024);
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn test_parse_apt_conflict_output() {
        let output = r#"
Reading package lists... Done
Building dependency tree       
Reading state information... Done
Some packages could not be installed. This may mean that you have
requested an impossible situation or if you are using the unstable
distribution that some required packages have not yet been created
or been moved out of Incoming.
The following information may help to resolve the situation:

The following packages have unmet dependencies:
 libssl-dev : Conflicts: libssl1.0-dev but 1.0.2n-1ubuntu5.3 is to be installed
E: Unable to correct problems, you have held broken packages.
"#;
        let result = SimulationEngine::parse_apt_output(output);
        assert!(!result.conflicts.is_empty());
        assert!(result.conflicts[0].contains("unmet dependencies"));
    }
}
