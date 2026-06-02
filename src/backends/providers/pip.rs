use async_trait::async_trait;
use std::process::Command;
use std::time::Duration;

use crate::errors::Result;
use crate::models::{Package, PackageSource};
use crate::traits::PackageProvider;

/// Pip package provider implementation
pub struct PipProvider {
    client: reqwest::Client,
}

impl Default for PipProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl PipProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("metapak/0.1.0")
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to create Pip HTTP client: {}, using default", e);
                reqwest::Client::new()
            });
        Self { client }
    }
}

#[async_trait]
impl PackageProvider for PipProvider {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        // Try exact match via JSON API first since pip search is deprecated
        let url = format!("https://pypi.org/pypi/{}/json", urlencoding::encode(query));

        let response = self.client.get(&url).send().await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<PypiResponse>().await {
                    let info = data.info;
                    return Ok(vec![Package {
                        name: info.name,
                        version: info.version,
                        description: info.summary.unwrap_or_default(),
                        source: PackageSource::Pip,
                        url: info.project_url.or(info.home_page),
                        maintainers: info.author.map(|a| vec![a]).unwrap_or_default(),
                        licenses: info.license.map(|l| vec![l]).unwrap_or_default(),
                        ..Default::default()
                    }]);
                }
            }
            _ => {}
        }

        Ok(Vec::new())
    }

    async fn is_installed(&self, pkg_name: &str) -> bool {
        let pkg_name = pkg_name.to_string();
        match tokio::task::spawn_blocking(move || {
            Command::new("pip")
                .args(["show", &pkg_name])
                .output()
                .map(|o| o.status.success())
        })
        .await
        {
            Ok(Ok(res)) => res,
            _ => false,
        }
    }
}

#[derive(serde::Deserialize, Debug)]
pub struct PypiResponse {
    pub info: PypiInfo,
}

#[derive(serde::Deserialize, Debug)]
pub struct PypiInfo {
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    pub home_page: Option<String>,
    pub project_url: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
}
