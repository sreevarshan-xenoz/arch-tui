use async_trait::async_trait;
use std::process::Command;

use crate::errors::{AppError, Result};
use crate::models::{Package, PackageSource};
use crate::traits::PackageProvider;

/// Cargo package provider implementation
pub struct CargoProvider;

impl Default for CargoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CargoProvider {
    pub fn new() -> Self {
        Self
    }

    pub fn parse_cargo_search(stdout: &str) -> Vec<Package> {
        let mut packages = Vec::new();
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("...") {
                continue;
            }

            // Format: name = "version" # description
            if let Some((name_ver, desc)) = line.split_once(" # ") {
                if let Some((name, ver)) = name_ver.split_once(" = ") {
                    let name = name.trim().to_string();
                    let version = ver.trim().trim_matches('"').to_string();
                    packages.push(Package {
                        name,
                        version,
                        description: desc.trim().to_string(),
                        source: PackageSource::Cargo,
                        ..Default::default()
                    });
                }
            } else if let Some((name, ver)) = line.split_once(" = ") {
                let name = name.trim().to_string();
                let version = ver.trim().trim_matches('"').to_string();
                packages.push(Package {
                    name,
                    version,
                    description: String::new(),
                    source: PackageSource::Cargo,
                    ..Default::default()
                });
            }
        }
        packages
    }
}

#[async_trait]
impl PackageProvider for CargoProvider {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let query = query.to_string();
        tokio::task::spawn_blocking(move || {
            let output = Command::new("cargo")
                .args(["search", &query, "--limit", "50"])
                .output()
                .map_err(|e| AppError::Cargo(format!("Failed to execute cargo search: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(AppError::Cargo(format!("cargo search failed: {}", stderr)));
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(Self::parse_cargo_search(&stdout))
        })
        .await
        .map_err(|e| AppError::Other(format!("Join error: {}", e)))?
    }

    async fn is_installed(&self, pkg_name: &str) -> bool {
        let pkg_name = pkg_name.to_string();
        let handle = tokio::task::spawn_blocking(move || {
            let output = Command::new("cargo").args(["install", "--list"]).output();

            if let Ok(o) = output {
                let stdout = String::from_utf8_lossy(&o.stdout);
                for line in stdout.lines() {
                    if line.starts_with(&format!("{} ", pkg_name)) {
                        return true;
                    }
                }
            }
            false
        });

        match handle.await {
            Ok(res) => res,
            Err(_) => false,
        }
    }
}
