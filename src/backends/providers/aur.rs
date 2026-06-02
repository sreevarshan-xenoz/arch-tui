use async_trait::async_trait;
use once_cell::sync::Lazy;
use std::process::Command;
use std::time::Duration;

use crate::errors::{AppError, Result};
use crate::models::{Package, PackageSource};
use crate::traits::PackageProvider;
use crate::utils::CircuitBreaker;

/// Circuit breaker for AUR API
static AUR_CIRCUIT_BREAKER: Lazy<CircuitBreaker> = Lazy::new(CircuitBreaker::new);

/// AUR package provider implementation
pub struct AurProvider {
    client: reqwest::Client,
}

impl Default for AurProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AurProvider {
    pub fn new() -> Self {
        use crate::constants::network::{
            AUR_CONNECT_TIMEOUT_SECS, AUR_REQUEST_TIMEOUT_SECS, HTTP_IDLE_TIMEOUT_SECS,
            HTTP_MAX_CONNECTIONS,
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(AUR_REQUEST_TIMEOUT_SECS))
            .connect_timeout(Duration::from_secs(AUR_CONNECT_TIMEOUT_SECS))
            .pool_max_idle_per_host(HTTP_MAX_CONNECTIONS as usize)
            .pool_idle_timeout(Duration::from_secs(HTTP_IDLE_TIMEOUT_SECS))
            .tcp_keepalive(Duration::from_secs(60))
            .tcp_nodelay(true)
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to create optimized HTTP client: {}, using default",
                    e
                );
                reqwest::Client::new()
            });
        Self { client }
    }
}

#[async_trait]
impl PackageProvider for AurProvider {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        // Check circuit breaker first
        if !AUR_CIRCUIT_BREAKER.is_available() {
            tracing::warn!("AUR circuit breaker is open, skipping request");
            return Err(AppError::Aur(
                "AUR service temporarily unavailable (circuit breaker open)".to_string(),
            ));
        }

        let url = format!(
            "https://aur.archlinux.org/rpc/v5/search/{}",
            urlencoding::encode(query)
        );

        const MAX_RETRIES: usize = 3;
        let mut response = None;
        let mut last_error = None;
        for attempt in 0..MAX_RETRIES {
            match self
                .client
                .get(&url)
                .header("User-Agent", "metapak/0.1.0")
                .send()
                .await
            {
                Ok(resp) => {
                    response = Some(resp);
                    break;
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                    if attempt + 1 < MAX_RETRIES {
                        tokio::time::sleep(Duration::from_millis(250 * (attempt as u64 + 1))).await;
                    }
                }
            }
        }

        // Record result in circuit breaker
        if response.is_none() {
            AUR_CIRCUIT_BREAKER.record_failure();
        } else {
            AUR_CIRCUIT_BREAKER.record_success();
        }

        let response = response.ok_or_else(|| {
            AppError::Aur(format!(
                "Failed to send AUR request after {} attempts: {}",
                MAX_RETRIES,
                last_error.unwrap_or_else(|| "unknown error".to_string())
            ))
        })?;

        if !response.status().is_success() {
            return Err(AppError::Aur(format!(
                "AUR request failed with status {}",
                response.status()
            )));
        }

        let aur_response: AurResponse = response
            .json()
            .await
            .map_err(|e| AppError::Aur(format!("Failed to parse AUR response: {}", e)))?;

        let packages: Vec<Package> = aur_response
            .results
            .into_iter()
            .map(|aur_pkg| {
                let mut all_deps = Vec::new();
                if let Some(depends) = aur_pkg.depends {
                    all_deps.extend(depends);
                }
                if let Some(make_depends) = aur_pkg.make_depends {
                    all_deps.extend(make_depends);
                }

                let is_outdated = aur_pkg.out_of_date.is_some();
                let package_base_id = aur_pkg.package_base_id.map(|id| id.to_string());

                Package {
                    name: aur_pkg.name,
                    version: aur_pkg.version,
                    description: aur_pkg.description.unwrap_or_default(),
                    source: PackageSource::Aur,
                    is_installed: false,
                    is_outdated,
                    installed_size: None,
                    download_size: None,
                    groups: vec![],
                    licenses: aur_pkg.licenses.unwrap_or_default(),
                    maintainers: aur_pkg.maintainer.map(|m| vec![m]).unwrap_or_default(),
                    keywords: aur_pkg.keywords.unwrap_or_default(),
                    url: aur_pkg.url,
                    depends_on: all_deps,
                    required_by: vec![],
                    opt_depends: aur_pkg.opt_depends.unwrap_or_default(),
                    conflicts: aur_pkg.conflicts.unwrap_or_default(),
                    replaces: vec![],
                    provides: aur_pkg.provides.unwrap_or_default(),
                    votes: aur_pkg.num_votes,
                    popularity: aur_pkg.popularity,
                    first_submitted: aur_pkg.first_submitted,
                    last_updated: aur_pkg.last_updated,
                    package_base_id,
                    num_votes: aur_pkg.num_votes,
                }
            })
            .collect();

        Ok(packages)
    }

    async fn is_installed(&self, pkg_name: &str) -> bool {
        let pkg_name = pkg_name.to_string();
        match tokio::task::spawn_blocking(move || {
            Command::new("pacman")
                .arg("-Qm")
                .arg(&pkg_name)
                .output()
                .map(|o| o.status.success())
        })
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => {
                tracing::warn!("Failed to check AUR package installation status: {}", e);
                false
            }
            Err(e) => {
                tracing::warn!("Failed to join AUR is_installed task: {}", e);
                false
            }
        }
    }
}

// AUR Response structures
#[derive(serde::Deserialize, Debug)]
struct AurResponse {
    #[serde(rename = "resultcount")]
    _result_count: u32,
    results: Vec<AurPackage>,
}

#[derive(serde::Deserialize, Debug)]
struct AurPackage {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "Maintainer")]
    maintainer: Option<String>,
    #[serde(rename = "Depends")]
    depends: Option<Vec<String>>,
    #[serde(rename = "MakeDepends")]
    make_depends: Option<Vec<String>>,
    #[serde(rename = "OptDepends")]
    opt_depends: Option<Vec<String>>,
    #[serde(rename = "Conflicts")]
    conflicts: Option<Vec<String>>,
    #[serde(rename = "License")]
    licenses: Option<Vec<String>>,
    #[serde(rename = "Keywords")]
    keywords: Option<Vec<String>>,
    #[serde(rename = "Provides")]
    provides: Option<Vec<String>>,
    #[serde(rename = "NumVotes")]
    num_votes: Option<i32>,
    #[serde(rename = "Popularity")]
    popularity: Option<f32>,
    #[serde(rename = "LastUpdated")]
    last_updated: Option<i64>,
    #[serde(rename = "FirstSubmitted")]
    first_submitted: Option<i64>,
    #[serde(rename = "OutOfDate")]
    out_of_date: Option<i64>,
    #[serde(rename = "PackageBaseID")]
    package_base_id: Option<i32>,
}
