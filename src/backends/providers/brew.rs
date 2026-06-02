use async_trait::async_trait;
use std::process::Command;

use crate::errors::{AppError, Result};
use crate::models::{OutdatedPackage, Package, PackageSource};
use crate::traits::{PackageProvider, UpdateProvider};

pub struct BrewProvider;

impl BrewProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PackageProvider for BrewProvider {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let query = query.to_string();
        tokio::task::spawn_blocking(move || {
            let output = Command::new("brew")
                .args(["search", &query])
                .output()
                .map_err(|e| AppError::Other(format!("Failed to execute brew search: {}", e)))?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut packages = Vec::new();
            for line in stdout.lines() {
                let name = line.trim();
                if !name.is_empty() && !name.contains("==>") {
                    packages.push(Package {
                        name: name.to_string(),
                        version: "latest".to_string(),
                        source: PackageSource::Brew,
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
            let output = Command::new("brew")
                .args(["list", "--formula", &pkg_name])
                .output();
            
            match output {
                Ok(o) => o.status.success(),
                Err(_) => false,
            }
        })
        .await
        .unwrap_or(false)
    }
}

#[async_trait]
impl UpdateProvider for BrewProvider {
    async fn check_updates(&self) -> Result<usize> {
        Ok(0)
    }

    async fn get_outdated_packages(&self) -> Result<Vec<OutdatedPackage>> {
        Ok(Vec::new())
    }
}
