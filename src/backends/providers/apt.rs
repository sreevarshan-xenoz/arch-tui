use async_trait::async_trait;
use std::process::Command;

use crate::errors::{AppError, Result};
use crate::models::{OutdatedPackage, Package, PackageSource};
use crate::traits::{PackageProvider, UpdateProvider};

pub struct AptProvider;

impl AptProvider {
    pub fn new() -> Self {
        Self
    }

    fn parse_line(line: &str) -> Option<Package> {
        // Format: name/repo version architecture [status]
        // Example: vim/stable,now 2:9.0.1378-2 amd64 [installed]
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            return None;
        }

        let name_repo = parts[0];
        let version = parts[1];
        let is_installed = line.contains("[installed]");

        let name = name_repo.split('/').next().unwrap_or(name_repo).to_string();

        Some(Package {
            name,
            version: version.to_string(),
            description: String::new(), // Description requires separate apt-cache show
            source: PackageSource::Apt,
            is_installed,
            ..Default::default()
        })
    }
}

#[async_trait]
impl PackageProvider for AptProvider {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let query = query.to_string();
        tokio::task::spawn_blocking(move || {
            let output = Command::new("apt")
                .args(["list", &format!("*{}*", query)])
                .output()
                .map_err(|e| AppError::Other(format!("Failed to execute apt list: {}", e)))?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut packages = Vec::new();
            for line in stdout.lines().skip(1) { // Skip "Listing..."
                if let Some(pkg) = Self::parse_line(line) {
                    packages.push(pkg);
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
            let output = Command::new("dpkg")
                .args(["-l", &pkg_name])
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
impl UpdateProvider for AptProvider {
    async fn check_updates(&self) -> Result<usize> {
        Ok(0) // Placeholder
    }

    async fn get_outdated_packages(&self) -> Result<Vec<OutdatedPackage>> {
        Ok(Vec::new()) // Placeholder
    }
}
