use async_trait::async_trait;
use std::process::Command;

use crate::errors::{AppError, Result};
use crate::models::{OutdatedPackage, Package, PackageSource};
use crate::traits::{PackageProvider, UpdateProvider};

/// Pacman package provider implementation
pub struct PacmanProvider;

impl Default for PacmanProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl PacmanProvider {
    pub fn new() -> Self {
        Self
    }

    /// Blocking search implementation
    pub fn search_blocking(query: &str) -> Result<Vec<Package>> {
        let output = Command::new("pacman")
            .arg("-Ss")
            .arg(query)
            .output()
            .map_err(|e| AppError::Pacman(format!("Failed to execute pacman search: {}", e)))?;

        if !output.status.success() {
            // Check if it's just no results vs an actual error
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("no results") || output.status.code() == Some(1) {
                return Ok(Vec::new());
            }
            return Err(AppError::Pacman(format!(
                "pacman search failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8(output.stdout)
            .map_err(|e| AppError::Pacman(format!("Invalid UTF-8 in pacman output: {}", e)))?;

        let mut packages = Vec::new();
        let mut lines = stdout.lines();

        while let Some(header) = lines.next() {
            if let Some(desc) = lines.next() {
                if let Some(pkg) = Self::parse_entry(header, desc) {
                    packages.push(pkg);
                }
            }
        }

        Ok(packages)
    }

    /// Parse a pacman package entry from command output
    fn parse_entry(header: &str, desc: &str) -> Option<Package> {
        // Header format: repo/name version (groups) [installed]
        // Example: core/linux 6.6.1-arch1 (base) [installed]
        let parts: Vec<&str> = header.split_whitespace().collect();
        if parts.len() < 2 {
            return None;
        }

        let full_name = parts[0]; // repo/name
        let version = parts[1];
        let is_installed = header.contains("[installed]") || header.contains("[Installed]");

        let name = full_name.split('/').nth(1).unwrap_or(full_name).to_string();

        Some(Package {
            name,
            version: version.to_string(),
            description: desc.trim().to_string(),
            source: PackageSource::Pacman,
            is_installed,
            is_outdated: false,
            installed_size: None,
            download_size: None,
            groups: vec![],
            licenses: vec![],
            maintainers: vec![],
            keywords: vec![],
            url: None,
            depends_on: vec![],
            required_by: vec![],
            opt_depends: vec![],
            conflicts: vec![],
            replaces: vec![],
            provides: vec![],
            votes: None,
            popularity: None,
            first_submitted: None,
            last_updated: None,
            package_base_id: None,
            num_votes: None,
        })
    }
}

#[async_trait]
impl PackageProvider for PacmanProvider {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let query = query.to_string();
        tokio::task::spawn_blocking(move || Self::search_blocking(&query))
            .await
            .map_err(|e| AppError::Other(format!("Join error: {}", e)))?
    }

    async fn is_installed(&self, pkg_name: &str) -> bool {
        let pkg_name = pkg_name.to_string();
        match tokio::task::spawn_blocking(move || {
            Command::new("pacman")
                .arg("-Qi")
                .arg(&pkg_name)
                .output()
                .map(|o| o.status.success())
        })
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => {
                tracing::warn!("Failed to check if package is installed: {}", e);
                false
            }
            Err(e) => {
                tracing::warn!("Failed to join is_installed task: {}", e);
                false
            }
        }
    }
}

#[async_trait]
impl UpdateProvider for PacmanProvider {
    async fn check_updates(&self) -> Result<usize> {
        tokio::task::spawn_blocking(move || {
            // Try checkupdates first (from pacman-contrib) - doesn't require sudo
            if let Ok(output) = Command::new("checkupdates").output() {
                if output.status.success() {
                    let stdout = String::from_utf8(output.stdout).map_err(|e| {
                        AppError::Pacman(format!("Invalid UTF-8 in checkupdates output: {}", e))
                    })?;
                    return Ok(stdout.lines().filter(|l| !l.is_empty()).count());
                }
            }

            // Fallback to pacman -Qu (checks against local DB)
            let output = Command::new("pacman")
                .arg("-Qu")
                .output()
                .map_err(|e| AppError::Pacman(format!("Failed to execute pacman -Qu: {}", e)))?;

            if output.status.success() {
                let stdout = String::from_utf8(output.stdout).map_err(|e| {
                    AppError::Pacman(format!("Invalid UTF-8 in pacman -Qu output: {}", e))
                })?;
                return Ok(stdout.lines().filter(|l| !l.is_empty()).count());
            }

            Ok(0)
        })
        .await
        .map_err(|e| AppError::Other(format!("Join error: {}", e)))?
    }

    async fn get_outdated_packages(&self) -> Result<Vec<OutdatedPackage>> {
        tokio::task::spawn_blocking(move || {
            let mut outdated = Vec::new();

            // Try pacman -Qu first (installed packages with newer versions available)
            let output = Command::new("pacman")
                .arg("-Qu")
                .output()
                .map_err(|e| AppError::Pacman(format!("Failed to execute pacman -Qu: {}", e)))?;

            if output.status.success() {
                let stdout = String::from_utf8(output.stdout).map_err(|e| {
                    AppError::Pacman(format!("Invalid UTF-8 in pacman -Qu output: {}", e))
                })?;

                for line in stdout.lines().filter(|l| !l.is_empty()) {
                    let parts: Vec<&str> = line.splitn(3, ' ').collect();
                    if parts.len() >= 2 {
                        let name = parts[0].to_string();
                        let version = parts[1].to_string();

                        let mut pkg = OutdatedPackage::new(
                            name.clone(),
                            "?".to_string(),
                            version,
                            "unknown".to_string(),
                        );

                        // Get package info
                        if let Ok(info) = Command::new("pacman").arg("-Qi").arg(&name).output() {
                            if info.status.success() {
                                let info_str = String::from_utf8_lossy(&info.stdout);
                                for info_line in info_str.lines() {
                                    if info_line.starts_with("Repository") {
                                        if let Some(repo) = info_line.split(':').nth(1) {
                                            pkg.repository = repo.trim().to_string();
                                            pkg.is_aur = pkg.repository.to_lowercase() == "aur";
                                        }
                                    } else if info_line.starts_with("Installed Size") {
                                        if let Some(size) = info_line.split(':').nth(1) {
                                            let size_str = size.trim();
                                            // Parse size like "150.00 MiB"
                                            let multiplier: u64 = if size_str.contains("GiB") {
                                                1024 * 1024
                                            } else if size_str.contains("MiB") {
                                                1024
                                            } else {
                                                1
                                            };
                                            let num: f64 = size_str
                                                .replace("GiB", "")
                                                .replace("MiB", "")
                                                .replace("KiB", "")
                                                .trim()
                                                .parse()
                                                .unwrap_or(0.0);
                                            pkg.download_size = (num * multiplier as f64) as u64;
                                        }
                                    } else if info_line.starts_with("Depends On") {
                                        let deps = info_line.split(':').nth(1).unwrap_or("");
                                        pkg.new_dependencies = deps
                                            .split_whitespace()
                                            .map(|s| s.to_string())
                                            .collect();
                                    }
                                }
                            }
                        }

                        outdated.push(pkg);
                    }
                }
            }

            Ok(outdated)
        })
        .await
        .map_err(|e| AppError::Other(format!("Join error: {}", e)))?
    }
}
