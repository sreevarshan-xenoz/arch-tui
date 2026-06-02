use async_trait::async_trait;
use std::process::Command;

use crate::errors::{AppError, Result};
use crate::models::{OutdatedPackage, Package, PackageSource};
use crate::traits::{PackageProvider, UpdateProvider};

pub struct ScoopProvider;

impl ScoopProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PackageProvider for ScoopProvider {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let query = query.to_string();
        tokio::task::spawn_blocking(move || {
            let output = Command::new("scoop")
                .args(["search", &query])
                .output()
                .map_err(|e| AppError::Other(format!("Failed to execute scoop search: {}", e)))?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut packages = Vec::new();
            for line in stdout.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with("'") || line.contains("Results from") {
                    continue;
                }
                
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    packages.push(Package {
                        name: parts[0].to_string(),
                        version: parts[1].to_string(),
                        source: PackageSource::Scoop,
                        ..Default::default()
                    });
                }
            }
            Ok(packages)
        })
        .await
        .map_err(|e| AppError::Other(format!("Join error: {}", e)))?
    }

    async fn is_installed(&self, pkg_name: &str) -> bool {
        let pkg_name = pkg_name.to_string();
        tokio::task::spawn_blocking(move || {
            let output = Command::new("scoop")
                .args(["list", &pkg_name])
                .output();
            
            match output {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    stdout.contains(&pkg_name)
                },
                Err(_) => false,
            }
        })
        .await
        .unwrap_or(false)
    }
}

#[async_trait]
impl UpdateProvider for ScoopProvider {
    async fn check_updates(&self) -> Result<usize> {
        Ok(0)
    }

    async fn get_outdated_packages(&self) -> Result<Vec<OutdatedPackage>> {
        Ok(Vec::new())
    }
}
