use async_trait::async_trait;
use std::time::Duration;

use crate::errors::{AppError, Result};
use crate::models::{Package, PackageSource};
use crate::traits::PackageProvider;

/// NPM package provider implementation
pub struct NpmProvider {
    client: reqwest::Client,
}

impl Default for NpmProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl NpmProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("metapak/0.1.0")
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to create NPM HTTP client: {}, using default", e);
                reqwest::Client::new()
            });
        Self { client }
    }

    pub fn parse_npm_response(npm_response: NpmResponse) -> Vec<Package> {
        npm_response
            .objects
            .into_iter()
            .map(|obj| {
                let pkg = obj.package;
                let mut keywords = pkg.keywords.unwrap_or_default();
                keywords.push("npm".to_string());

                let mut maintainers = Vec::new();
                if let Some(publisher) = pkg.publisher {
                    if let Some(user) = publisher.username {
                        maintainers.push(user);
                    }
                }

                if let Some(npm_maintainers) = pkg.maintainers {
                    for m in npm_maintainers {
                        if let Some(username) = m.username {
                            if !maintainers.contains(&username) {
                                maintainers.push(username);
                            }
                        }
                    }
                }

                let mut licenses = Vec::new();
                if let Some(license) = pkg.license {
                    licenses.push(license);
                }

                Package {
                    name: pkg.name,
                    version: pkg.version,
                    description: pkg.description.unwrap_or_default(),
                    source: PackageSource::Npm,
                    is_installed: false,
                    is_outdated: false,
                    installed_size: None,
                    download_size: None,
                    groups: vec![],
                    licenses,
                    maintainers,
                    keywords,
                    url: pkg.links.and_then(|l| l.npm),
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
                }
            })
            .collect()
    }
}

#[async_trait]
impl PackageProvider for NpmProvider {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let url = format!(
            "https://registry.npmjs.org/-/v1/search?text={}&size=40",
            urlencoding::encode(query)
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Other(format!("NPM request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::Other(format!(
                "NPM search failed with status {}",
                response.status()
            )));
        }

        let npm_response: NpmResponse = response
            .json()
            .await
            .map_err(|e| AppError::Other(format!("Failed to parse NPM response: {}", e)))?;

        Ok(Self::parse_npm_response(npm_response))
    }

    async fn is_installed(&self, _pkg_name: &str) -> bool {
        // Implementation for checking global NPM packages could be added
        false
    }
}

// NPM Response structures
#[derive(serde::Deserialize, Debug)]
pub struct NpmResponse {
    pub objects: Vec<NpmResult>,
}

#[derive(serde::Deserialize, Debug)]
pub struct NpmResult {
    pub package: NpmPackage,
}

#[derive(serde::Deserialize, Debug)]
pub struct NpmPackage {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub links: Option<NpmLinks>,
    pub publisher: Option<NpmUser>,
    pub maintainers: Option<Vec<NpmUser>>,
    pub license: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct NpmLinks {
    pub npm: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct NpmUser {
    pub username: Option<String>,
}
