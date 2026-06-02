//! Service layer for package management operations
//!
//! This module provides async implementations for package management operations
//! following the provider traits pattern for better testability and organization.

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::config::AppConfig;
use crate::errors::{AppError, Result};
use crate::models::{OutdatedPackage, Package, PackageSource};
use crate::search::EnhancedSearch;
use crate::traits::{PackageProvider, UpdateProvider};

// Import providers from backends
pub use crate::backends::providers::apk::ApkProvider;
pub use crate::backends::providers::apt::AptProvider;
pub use crate::backends::providers::aur::AurProvider;
pub use crate::backends::providers::brew::BrewProvider;
pub use crate::backends::providers::cargo::CargoProvider;
pub use crate::backends::providers::dnf::DnfProvider;
pub use crate::backends::providers::npm::NpmProvider;
pub use crate::backends::providers::pacman::PacmanProvider;
pub use crate::backends::providers::pip::PipProvider;
pub use crate::backends::providers::scoop::ScoopProvider;
pub use crate::backends::providers::zypper::ZypperProvider;

/// Thread-safe search result cache
pub struct SearchCache {
    map: RwLock<HashMap<String, (Vec<Package>, Instant)>>,
}

impl SearchCache {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
        }
    }

    pub fn get_cached(&self, query: &str, ttl: Duration) -> Option<Vec<Package>> {
        let map = self.map.read().ok()?;
        if let Some((results, timestamp)) = map.get(query) {
            if timestamp.elapsed() < ttl {
                return Some(results.clone());
            }
        }
        None
    }

    pub fn put_cached(&self, query: &str, results: Vec<Package>) {
        if let Ok(mut map) = self.map.write() {
            map.insert(query.to_string(), (results, Instant::now()));
        }
    }

    #[allow(dead_code)]
    pub fn clear(&self) {
        if let Ok(mut map) = self.map.write() {
            map.clear();
        }
    }
}

impl Default for SearchCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Update provider implementation that delegates to the system provider
pub struct SystemUpdateProvider {
    inner: Arc<dyn UpdateProvider>,
}

impl SystemUpdateProvider {
    pub fn new(inner: Arc<dyn UpdateProvider>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl UpdateProvider for SystemUpdateProvider {
    async fn check_updates(&self) -> Result<usize> {
        self.inner.check_updates().await
    }

    async fn get_outdated_packages(&self) -> Result<Vec<OutdatedPackage>> {
        self.inner.get_outdated_packages().await
    }
}

/// Package service that orchestrates multiple providers
pub struct PackageService {
    providers: Vec<Arc<dyn PackageProvider>>,
    update_provider: Arc<dyn UpdateProvider>,
    cache: Arc<SearchCache>,
    #[allow(dead_code)]
    config: AppConfig,
}

#[derive(Debug, Clone, PartialEq)]
enum QuerySourceFilter {
    Any,
    Repo,
    Aur,
}

#[derive(Debug, Clone)]
struct QuerySpec {
    text: String,
    source: QuerySourceFilter,
    installed: Option<bool>,
    name_prefix: Option<String>,
    name_contains: Option<String>,
    dependency_contains: Option<String>,
}

impl QuerySpec {
    fn parse(input: &str) -> Self {
        let mut text_tokens = Vec::new();
        let mut spec = Self {
            text: String::new(),
            source: QuerySourceFilter::Any,
            installed: None,
            name_prefix: None,
            name_contains: None,
            dependency_contains: None,
        };

        for token in input.split_whitespace() {
            if let Some((k, v)) = token.split_once(':') {
                match k {
                    "source" => {
                        spec.source = match v {
                            "aur" => QuerySourceFilter::Aur,
                            "repo" | "pacman" => QuerySourceFilter::Repo,
                            _ => QuerySourceFilter::Any,
                        };
                    }
                    "installed" => match v {
                        "true" | "yes" | "1" => spec.installed = Some(true),
                        "false" | "no" | "0" => spec.installed = Some(false),
                        _ => {}
                    },
                    "name" => {
                        if let Some(rest) = v.strip_prefix('^') {
                            if !rest.is_empty() {
                                spec.name_prefix = Some(rest.to_lowercase());
                            }
                        } else if !v.is_empty() {
                            spec.name_contains = Some(v.to_lowercase());
                        }
                    }
                    "dep" | "depends" => {
                        if !v.is_empty() {
                            spec.dependency_contains = Some(v.to_lowercase());
                        }
                    }
                    _ => text_tokens.push(token.to_string()),
                }
            } else {
                text_tokens.push(token.to_string());
            }
        }

        spec.text = text_tokens.join(" ").trim().to_string();
        spec
    }
}

#[allow(dead_code)]
pub fn command_display(spec: &CommandSpec) -> String {
    format!("{} {}", spec.prog, spec.args.join(" "))
}

pub fn plan_package_transaction(packages: &[Package], config: &AppConfig) -> Vec<CommandSpec> {
    let mut commands = Vec::new();
    let helper = AurHelperCommand::new(config);

    // Group by source and action (install/remove)
    let mut to_install: HashMap<PackageSource, Vec<String>> = HashMap::new();
    let mut to_remove: HashMap<PackageSource, Vec<String>> = HashMap::new();

    for pkg in packages {
        if pkg.is_installed {
            to_remove
                .entry(pkg.source.clone())
                .or_default()
                .push(pkg.name.clone());
        } else {
            to_install
                .entry(pkg.source.clone())
                .or_default()
                .push(pkg.name.clone());
        }
    }

    // Handle removals
    for (source, names) in to_remove {
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        match source {
            PackageSource::Pacman | PackageSource::Aur => {
                commands.push(helper.remove_command(&name_refs));
            }
            PackageSource::Npm => {
                let mut args = vec!["uninstall".to_string(), "-g".to_string()];
                args.extend(names);
                commands.push(CommandSpec::new_no_sudo("npm", args));
            }
            PackageSource::Cargo => {
                let mut args = vec!["uninstall".to_string()];
                args.extend(names);
                commands.push(CommandSpec::new_no_sudo("cargo", args));
            }
            PackageSource::Pip => {
                let mut args = vec!["uninstall".to_string(), "-y".to_string()];
                args.extend(names);
                commands.push(CommandSpec::new_no_sudo("pip", args));
            }
            _ => {
                // Implementation for other sources
            }
        }
    }

    // Handle installs
    for (source, names) in to_install {
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        match source {
            PackageSource::Pacman => {
                let mut args = vec![
                    "pacman".to_string(),
                    "-S".to_string(),
                    "--noconfirm".to_string(),
                ];
                args.extend(names);
                commands.push(CommandSpec::new("sudo", args));
            }
            PackageSource::Aur => {
                commands.push(helper.install_command(&name_refs));
            }
            PackageSource::Npm => {
                let mut args = vec!["install".to_string(), "-g".to_string()];
                args.extend(names);
                commands.push(CommandSpec::new_no_sudo("npm", args));
            }
            PackageSource::Cargo => {
                let mut args = vec!["install".to_string()];
                args.extend(names);
                commands.push(CommandSpec::new_no_sudo("cargo", args));
            }
            PackageSource::Pip => {
                let mut args = vec!["install".to_string(), "--user".to_string()];
                args.extend(names);
                commands.push(CommandSpec::new_no_sudo("pip", args));
            }
            PackageSource::Apt => {
                 let mut args = vec!["apt-get".to_string(), "install".to_string(), "-y".to_string()];
                 args.extend(names);
                 commands.push(CommandSpec::new("sudo", args));
            }
            PackageSource::Brew => {
                 let mut args = vec!["install".to_string()];
                 args.extend(names);
                 commands.push(CommandSpec::new_no_sudo("brew", args));
            }
            PackageSource::Scoop => {
                 let mut args = vec!["install".to_string()];
                 args.extend(names);
                 commands.push(CommandSpec::new_no_sudo("scoop", args));
            }
            _ => {
                // Implementation for other sources
            }
        }
    }

    commands
}

pub fn plan_rollback_transaction(
    originally_installed: &[String],
    originally_removed: &[String],
    config: &AppConfig,
) -> Vec<CommandSpec> {
    let helper = AurHelperCommand::new(config);
    let mut commands = Vec::new();

    // Undo installs by removing them
    if !originally_installed.is_empty() {
        let names: Vec<&str> = originally_installed.iter().map(String::as_str).collect();
        commands.push(helper.remove_command(&names));
    }

    // Undo removals by re-installing them
    if !originally_removed.is_empty() {
        let names: Vec<&str> = originally_removed.iter().map(String::as_str).collect();
        commands.push(helper.install_command(&names));
    }

    commands
}

/// Global search cache shared across PackageService instances
static SEARCH_CACHE: Lazy<Arc<SearchCache>> = Lazy::new(|| Arc::new(SearchCache::new()));

impl PackageService {
    pub fn new(config: AppConfig) -> Self {
        let mut providers: Vec<Arc<dyn PackageProvider>> = Vec::new();
        
        // Detect system providers
        use crate::platform::{detect_package_managers, PackageManager};
        let managers = detect_package_managers();
        
        let mut update_provider: Option<Arc<dyn UpdateProvider>> = None;

        for manager in managers {
            match manager {
                PackageManager::Pacman => {
                    let p = Arc::new(PacmanProvider::new());
                    providers.push(p.clone());
                    providers.push(Arc::new(AurProvider::new()));
                    if update_provider.is_none() { update_provider = Some(p); }
                }
                PackageManager::Apt => {
                    let p = Arc::new(AptProvider::new());
                    providers.push(p.clone());
                    if update_provider.is_none() { update_provider = Some(p); }
                }
                PackageManager::Brew => {
                    let p = Arc::new(BrewProvider::new());
                    providers.push(p.clone());
                    if update_provider.is_none() { update_provider = Some(p); }
                }
                PackageManager::Scoop => {
                    let p = Arc::new(ScoopProvider::new());
                    providers.push(p.clone());
                    if update_provider.is_none() { update_provider = Some(p); }
                }
                PackageManager::Dnf => {
                    let p = Arc::new(DnfProvider::new());
                    providers.push(p.clone());
                    if update_provider.is_none() { update_provider = Some(p); }
                }
                PackageManager::Zypper => {
                    let p = Arc::new(ZypperProvider::new());
                    providers.push(p.clone());
                    if update_provider.is_none() { update_provider = Some(p); }
                }
                PackageManager::Apk => {
                    let p = Arc::new(ApkProvider::new());
                    providers.push(p.clone());
                    if update_provider.is_none() { update_provider = Some(p); }
                }
                _ => {}
            }
        }

        // Ecosystem providers (always added)
        providers.push(Arc::new(NpmProvider::new()));
        providers.push(Arc::new(CargoProvider::new()));
        providers.push(Arc::new(PipProvider::new()));

        // Fallback update provider if none detected
        let update_provider = update_provider.unwrap_or_else(|| Arc::new(PacmanProvider::new()));

        Self {
            providers,
            update_provider,
            cache: Arc::clone(&SEARCH_CACHE),
            config,
        }
    }

    /// Search across all providers concurrently
    pub async fn search_all(&self, query: &str) -> Result<Vec<Package>> {
        // Check cache first
        let ttl = Duration::from_secs(self.config.search.cache_ttl_seconds);
        if let Some(cached) = self.cache.get_cached(query, ttl) {
            tracing::debug!("Cache hit for search_all: {}", query);
            return Ok(cached);
        }

        let spec = QuerySpec::parse(query);
        if spec.text.trim().is_empty()
            && spec.installed.is_none()
            && spec.source == QuerySourceFilter::Any
            && spec.name_prefix.is_none()
            && spec.name_contains.is_none()
            && spec.dependency_contains.is_none()
        {
            return Ok(Vec::new());
        }

        let base_query = if spec.text.is_empty() {
            query.trim()
        } else {
            spec.text.as_str()
        };
        let mut all_results = Vec::new();
        let mut tasks = Vec::new();

        for provider in &self.providers {
            let provider = Arc::clone(provider);
            let query = base_query.to_string();
            let task = tokio::spawn(async move { provider.search(&query).await });
            tasks.push(task);
        }

        for task in tasks {
            match task.await {
                Ok(Ok(packages)) => all_results.extend(packages),
                Ok(Err(e)) => {
                    tracing::warn!("Provider search failed: {}", e);
                }
                Err(e) => {
                    tracing::error!("Task join error: {}", e);
                }
            }
        }

        // Update installation status for AUR packages
        let pacman = PacmanProvider::new();
        for pkg in &mut all_results {
            if pkg.source == PackageSource::Aur && !pkg.is_installed {
                pkg.is_installed = pacman.is_installed(&pkg.name).await;
            }
        }

        // Deduplicate by source+name and keep richer entries if duplicates exist.
        let mut deduped: HashMap<(String, String), Package> = HashMap::new();
        for pkg in all_results {
            let source = pkg.source.to_string();
            let key = (source, pkg.name.clone());
            deduped
                .entry(key)
                .and_modify(|existing| {
                    if !existing.is_installed && pkg.is_installed {
                        existing.is_installed = true;
                    }
                    if existing.description.is_empty() && !pkg.description.is_empty() {
                        existing.description = pkg.description.clone();
                    }
                })
                .or_insert(pkg);
        }

        let results: Vec<Package> = deduped.into_values().collect();

        // Use EnhancedSearch for filtering and ranking
        let search_engine = EnhancedSearch::new();
        let filtered = search_engine.filter_packages(&results, base_query);
        let mut results: Vec<Package> = filtered.into_iter().cloned().collect();

        results.sort_by(|a, b| {
            let a_name = a.name.to_lowercase();
            let b_name = b.name.to_lowercase();

            let a_score = search_engine
                .match_with_score(&a_name, base_query)
                .map(|(s, _)| s)
                .unwrap_or(0);
            let b_score = search_engine
                .match_with_score(&b_name, base_query)
                .map(|(s, _)| s)
                .unwrap_or(0);

            b_score
                .cmp(&a_score) // Higher score first
                .then_with(|| a_name.cmp(&b_name))
                .then_with(|| match (&a.source, &b.source) {
                    (PackageSource::Pacman, PackageSource::Aur) => std::cmp::Ordering::Less,
                    (PackageSource::Aur, PackageSource::Pacman) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                })
        });

        use crate::constants::search_limits::MAX_TOTAL_RESULTS;
        if results.len() > MAX_TOTAL_RESULTS {
            tracing::warn!(
                "Search results truncated from {} to {}",
                results.len(),
                MAX_TOTAL_RESULTS
            );
            results.truncate(MAX_TOTAL_RESULTS);
        }

        self.cache.put_cached(query, results.clone());

        Ok(results)
    }

    /// Search for available updates
    pub async fn check_updates(&self) -> Result<usize> {
        self.update_provider.check_updates().await
    }

    /// Search for packages in a specific ecosystem
    pub async fn search_ecosystem(
        &self,
        kind: crate::app::EcosystemKind,
        query: &str,
    ) -> Result<Vec<Package>> {
        let provider: Arc<dyn PackageProvider> = match kind {
            crate::app::EcosystemKind::Npm => Arc::new(NpmProvider::new()),
            crate::app::EcosystemKind::Cargo => Arc::new(CargoProvider::new()),
            crate::app::EcosystemKind::Pip => Arc::new(PipProvider::new()),
        };

        provider.search(query).await
    }

    pub fn scan_pacnew_pacsave() -> Result<Vec<PacnewPacsaveFile>> {
        let mut files = Vec::new();
        let etc_dir = std::path::Path::new("/etc");

        if !etc_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(etc_dir)
            .map_err(|e| AppError::Other(format!("Failed to read /etc: {}", e)))?;

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            let file_type = if let Some(ext) = path.extension() {
                if ext == "pacnew" {
                    Some(PacnewType::New)
                } else if ext == "pacsave" {
                    Some(PacnewType::Save)
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(ft) = file_type {
                let original_name = file_name
                    .strip_suffix(".pacnew")
                    .or_else(|| file_name.strip_suffix(".pacsave"))
                    .unwrap_or(file_name)
                    .to_string();

                let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

                files.push(PacnewPacsaveFile {
                    path: path.to_string_lossy().to_string(),
                    original_name,
                    file_type: ft,
                    modified,
                });
            }
        }

        files.sort_by(|a, b| a.original_name.cmp(&b.original_name));
        Ok(files)
    }

    pub fn read_pacman_log(limit: usize) -> Result<Vec<LogEntry>> {
        let log_path = std::path::Path::new("/var/log/pacman.log");
        if !log_path.exists() {
            return Err(AppError::Other("Pacman log not found".to_string()));
        }

        let content = std::fs::read_to_string(log_path)
            .map_err(|e| AppError::Other(format!("Failed to read log: {}", e)))?;

        let entries: Vec<LogEntry> = content
            .lines()
            .rev()
            .take(limit)
            .filter_map(Self::parse_log_line)
            .collect();

        Ok(entries)
    }

    fn parse_log_line(line: &str) -> Option<LogEntry> {
        // Format: [2026-05-07T10:30:45+0000] [ALPM] info: installed foo
        let line = line.trim();
        if !line.starts_with('[') {
            return None;
        }

        let mut parts = line.splitn(4, ']');
        let timestamp = parts.next()?.trim_start_matches('[').to_string();
        let rest = parts.next()?.trim();

        let operation = if rest.contains("installed") {
            LogOperation::Installed
        } else if rest.contains("removed") {
            LogOperation::Removed
        } else if rest.contains("upgraded") {
            LogOperation::Upgraded
        } else if rest.contains("downgraded") {
            LogOperation::Downgraded
        } else {
            return None;
        };

        // Extract package name from message like "installed foo"
        let msg = parts.next()?.trim();
        let package = msg.split_whitespace().last()?.to_string();

        Some(LogEntry {
            timestamp,
            operation,
            package,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PacnewPacsaveFile {
    pub path: String,
    pub original_name: String,
    pub file_type: PacnewType,
    #[allow(dead_code)]
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PacnewType {
    New,
    Save,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub operation: LogOperation,
    pub package: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogOperation {
    Installed,
    Removed,
    Upgraded,
    Downgraded,
}

#[derive(Debug, Clone)]
pub struct AvailableVersion {
    pub version: String,
    pub date: String,
    #[allow(dead_code)]
    pub repo: String,
}

impl PackageService {
    pub fn get_available_versions(pkg_name: &str) -> Result<Vec<AvailableVersion>> {
        let output = Command::new("curl")
            .args([
                "-s",
                &format!(
                    "https://archive.archlinux.org/packages/{}/{}/",
                    &pkg_name[0..1],
                    pkg_name
                ),
            ])
            .output();

        if output.is_err() {
            return Err(AppError::Other(
                "Failed to fetch package versions".to_string(),
            ));
        }

        let output = output.unwrap();
        let html = String::from_utf8_lossy(&output.stdout);
        let mut versions = Vec::new();

        let re = Regex::new(r#"href="(\d{4}/\d{2}/\d{2})/([^"]+/)""#).unwrap();
        for cap in re.captures_iter(&html) {
            if let (Some(date), Some(ver)) = (cap.get(1), cap.get(2)) {
                let version = ver.as_str().trim_end_matches('/').to_string();
                if !version.is_empty() {
                    versions.push(AvailableVersion {
                        version,
                        date: date.as_str().to_string(),
                        repo: "unknown".to_string(),
                    });
                }
            }
        }

        versions.sort_by(|a, b| b.version.cmp(&a.version));
        versions.truncate(20);

        Ok(versions)
    }

    pub fn build_downgrade_command(pkg_name: &str, version: &str) -> CommandSpec {
        CommandSpec {
            prog: "sudo".to_string(),
            args: vec![
                "pacman".to_string(),
                "-U".to_string(),
                "--noconfirm".to_string(),
                format!(
                    "https://archive.archlinux.org/packages/{}/{}/{}-{}.pkg.tar.zst",
                    &pkg_name[0..1],
                    pkg_name,
                    pkg_name,
                    version
                ),
            ],
        }
    }
}

impl Default for PackageService {
    fn default() -> Self {
        Self::new(AppConfig::default())
    }
}

/// Command builder for safe command execution
#[allow(dead_code)]
pub struct SafeCommandBuilder {
    program: String,
    args: Vec<String>,
}

impl SafeCommandBuilder {
    #[allow(dead_code)]
    pub fn new(program: &str) -> Self {
        Self {
            program: program.to_string(),
            args: Vec::new(),
        }
    }

    /// Sanitize and add an argument
    #[allow(dead_code)]
    pub fn arg(mut self, arg: &str) -> Self {
        let sanitized = Self::sanitize(arg);
        self.args.push(sanitized);
        self
    }

    /// Sanitize and add multiple arguments
    #[allow(dead_code)]
    pub fn args(mut self, args: &[&str]) -> Self {
        for arg in args {
            self.args.push(Self::sanitize(arg));
        }
        self
    }

    /// Build the command string for display
    #[allow(dead_code)]
    pub fn build_display(&self) -> String {
        format!("{} {}", self.program, self.args.join(" "))
    }

    /// Sanitize input to prevent command injection
    fn sanitize(input: &str) -> String {
        // Remove dangerous characters that could be used for injection
        let pattern = r##"[;&|<>$`"\n\r\x00]"##;
        match Regex::new(pattern) {
            Ok(dangerous) => dangerous.replace_all(input, "").to_string(),
            Err(_) => input.to_string(),
        }
    }
}

/// Structured command specification (program + arguments)
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub prog: String,
    pub args: Vec<String>,
}

impl CommandSpec {
    pub fn new(prog: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            prog: prog.into(),
            args,
        }
    }

    pub fn new_no_sudo(prog: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            prog: prog.into(),
            args,
        }
    }
}

/// AUR helper command builder
pub struct AurHelperCommand {
    helper: HelperKind,
}

#[derive(Debug, Clone, PartialEq)]
enum HelperKind {
    Paru,
    Yay,
    Pacman,
}

impl AurHelperCommand {
    const NOCONFIRM: &'static str = "--noconfirm";

    pub fn new(config: &AppConfig) -> Self {
        let helper = Self::detect_helper(&config.aur_helper);
        Self { helper }
    }

    fn sanitize_package_name(name: &str) -> String {
        name.chars()
            .filter(|c| c.is_alphanumeric() || "@._+-".contains(*c))
            .collect()
    }

    fn is_valid_package_name(name: &str) -> bool {
        !name.is_empty()
            && regex::Regex::new(r"^[a-z0-9@._+-]+$")
                .unwrap()
                .is_match(name)
    }

    fn detect_helper(configured: &str) -> HelperKind {
        match configured {
            "paru" => HelperKind::Paru,
            "yay" => HelperKind::Yay,
            "pacman" => HelperKind::Pacman,
            _ => {
                if Self::command_exists("paru") {
                    HelperKind::Paru
                } else if Self::command_exists("yay") {
                    HelperKind::Yay
                } else {
                    HelperKind::Pacman
                }
            }
        }
    }

    fn command_exists(cmd: &str) -> bool {
        #[cfg(target_os = "windows")]
        {
            Command::new("where")
                .arg(cmd)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        #[cfg(not(target_os = "windows"))]
        {
            Command::new("which")
                .arg(cmd)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    }

    /// Build install command
    pub fn install_command(&self, packages: &[&str]) -> CommandSpec {
        let mut args = vec!["-S".to_string(), Self::NOCONFIRM.to_string()];
        args.extend(packages.iter().map(|p| Self::sanitize_package_name(p)));

        match self.helper {
            HelperKind::Paru => CommandSpec::new("paru", args),
            HelperKind::Yay => CommandSpec::new("yay", args),
            HelperKind::Pacman => {
                let mut pacman_args = vec!["pacman".to_string()];
                pacman_args.extend(args);
                CommandSpec::new("sudo", pacman_args)
            }
        }
    }

    /// Build remove command
    pub fn remove_command(&self, packages: &[&str]) -> CommandSpec {
        let mut args = vec![
            "pacman".to_string(),
            "-Rns".to_string(),
            Self::NOCONFIRM.to_string(),
        ];
        args.extend(packages.iter().map(|p| Self::sanitize_package_name(p)));
        CommandSpec::new("sudo", args)
    }

    #[allow(dead_code)]
    /// Build install command with validation
    pub fn install_command_validated(
        &self,
        packages: &[&str],
    ) -> std::result::Result<CommandSpec, String> {
        for pkg in packages {
            if !Self::is_valid_package_name(pkg) {
                return Err(format!("Invalid package name: {}", pkg));
            }
        }
        Ok(self.install_command(packages))
    }

    #[allow(dead_code)]
    /// Build remove command with validation
    pub fn remove_command_validated(
        &self,
        packages: &[&str],
    ) -> std::result::Result<CommandSpec, String> {
        for pkg in packages {
            if !Self::is_valid_package_name(pkg) {
                return Err(format!("Invalid package name: {}", pkg));
            }
        }
        Ok(self.remove_command(packages))
    }

    /// Build update command
    pub fn update_command(&self) -> CommandSpec {
        let args = vec!["-Syu".to_string(), Self::NOCONFIRM.to_string()];
        match self.helper {
            HelperKind::Paru => CommandSpec::new("paru", args),
            HelperKind::Yay => CommandSpec::new("yay", args),
            HelperKind::Pacman => {
                let mut pacman_args = vec!["pacman".to_string()];
                pacman_args.extend(args);
                CommandSpec::new("sudo", pacman_args)
            }
        }
    }
}

pub fn copy_to_clipboard(text: &str) -> bool {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => clipboard.set_text(text).is_ok(),
        Err(_) => false,
    }
}

pub fn get_aur_clone_command(pkg_name: &str) -> String {
    format!("git clone https://aur.archlinux.org/{}.git", pkg_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_input() {
        assert_eq!(SafeCommandBuilder::sanitize("firefox"), "firefox");
        assert_eq!(SafeCommandBuilder::sanitize("rm -rf /"), "rm -rf /");
        assert_eq!(
            SafeCommandBuilder::sanitize("test; rm -rf /"),
            "test rm -rf /"
        );
        assert_eq!(
            SafeCommandBuilder::sanitize("test|cat /etc/passwd"),
            "testcat /etc/passwd"
        );
        assert_eq!(SafeCommandBuilder::sanitize("test`whoami`"), "testwhoami");
    }

    #[test]
    fn test_package_name_sanitization() {
        assert_eq!(
            AurHelperCommand::sanitize_package_name("firefox"),
            "firefox"
        );
        assert_eq!(
            AurHelperCommand::sanitize_package_name("linux-headers"),
            "linux-headers"
        );
        assert_eq!(
            AurHelperCommand::sanitize_package_name("python-pytest"),
            "python-pytest"
        );
        assert_eq!(
            AurHelperCommand::sanitize_package_name("pkg+name@test"),
            "pkg+name@test"
        );
        assert_eq!(
            AurHelperCommand::sanitize_package_name("test; rm -rf /"),
            "testrm-rf"
        );
    }

    #[test]
    fn test_package_name_validation() {
        assert!(AurHelperCommand::is_valid_package_name("firefox"));
        assert!(AurHelperCommand::is_valid_package_name("linux-headers"));
        assert!(AurHelperCommand::is_valid_package_name("python3"));
        assert!(AurHelperCommand::is_valid_package_name(
            "nodejs-lts-hydrogen"
        ));
        assert!(!AurHelperCommand::is_valid_package_name(""));
        assert!(!AurHelperCommand::is_valid_package_name("test; rm"));
        assert!(!AurHelperCommand::is_valid_package_name("test|grep"));
        assert!(!AurHelperCommand::is_valid_package_name("test\n"));
    }

    #[test]
    fn test_aur_helper_install_validated() {
        let config = AppConfig::default();
        let helper = AurHelperCommand::new(&config);

        let result = helper.install_command_validated(&["firefox", "vlc"]);
        assert!(result.is_ok());

        let result = helper.install_command_validated(&["test; rm -rf /"]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid package name"));
    }

    #[test]
    fn test_parse_pacman_entry() {
        let header = "core/linux 6.6.1-arch1 (base) [installed]";
        let desc = "The Linux kernel and modules";

        let pkg = PacmanProvider::parse_entry_for_test(header, desc);
        assert!(pkg.is_some());

        let pkg = pkg.unwrap();
        assert_eq!(pkg.name, "linux");
        assert_eq!(pkg.version, "6.6.1-arch1");
        assert!(pkg.is_installed);
    }

    #[test]
    fn test_parse_pacman_entry_not_installed() {
        let header = "community/firefox 120.0-1";
        let desc = "Standalone web browser";

        let pkg = PacmanProvider::parse_entry_for_test(header, desc);
        assert!(pkg.is_some());
        assert!(!pkg.unwrap().is_installed);
    }

    #[test]
    fn test_safe_command_builder() {
        let cmd = SafeCommandBuilder::new("pacman")
            .arg("-S")
            .args(&["firefox", "vlc"]);

        assert_eq!(cmd.build_display(), "pacman -S firefox vlc");
    }

    #[test]
    fn test_search_cache_hit_miss() {
        let cache = SearchCache::new();
        let query = "test-query";
        let pkg = Package::new("test-pkg", "1.0");
        let results = vec![pkg];
        let ttl = Duration::from_secs(60);

        // Miss initially
        assert!(cache.get_cached(query, ttl).is_none());

        // Put and Hit
        cache.put_cached(query, results.clone());
        let cached = cache.get_cached(query, ttl);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap()[0].name, "test-pkg");

        // Different query miss
        assert!(cache.get_cached("other", ttl).is_none());
    }

    #[test]
    fn test_search_cache_expiry() {
        let cache = SearchCache::new();
        let query = "expiry-query";
        let pkg = Package::new("test-pkg", "1.0");
        let results = vec![pkg];

        // Put with very short TTL
        cache.put_cached(query, results);

        // Immediate hit
        assert!(cache.get_cached(query, Duration::from_secs(10)).is_some());

        // Expired hit (using 0 TTL)
        assert!(cache.get_cached(query, Duration::from_secs(0)).is_none());
    }

    #[test]
    fn test_search_cache_clear() {
        let cache = SearchCache::new();
        let query = "clear-query";
        cache.put_cached(query, vec![Package::new("p", "1")]);

        cache.clear();
        assert!(cache.get_cached(query, Duration::from_secs(60)).is_none());
    }

    #[test]
    fn test_get_aur_clone_command() {
        let cmd = get_aur_clone_command("firefox");
        assert_eq!(cmd, "git clone https://aur.archlinux.org/firefox.git");

        let cmd = get_aur_clone_command("polybar");
        assert_eq!(cmd, "git clone https://aur.archlinux.org/polybar.git");
    }

    #[test]
    fn test_log_operation_enum() {
        assert_eq!(LogOperation::Installed, LogOperation::Installed);
        assert_eq!(LogOperation::Removed, LogOperation::Removed);
        assert_eq!(LogOperation::Upgraded, LogOperation::Upgraded);
        assert_eq!(LogOperation::Downgraded, LogOperation::Downgraded);
    }

    #[test]
    fn test_log_entry_creation() {
        let entry = LogEntry {
            timestamp: "2026-05-07T10:30:45+0000".to_string(),
            operation: LogOperation::Installed,
            package: "firefox".to_string(),
        };
        assert_eq!(entry.package, "firefox");
        assert_eq!(entry.operation, LogOperation::Installed);
    }

    #[test]
    fn test_pacnew_type_enum() {
        assert_eq!(PacnewType::New, PacnewType::New);
        assert_eq!(PacnewType::Save, PacnewType::Save);
    }

    #[test]
    fn test_command_spec_display() {
        let spec = CommandSpec {
            prog: "sudo".to_string(),
            args: vec![
                "pacman".to_string(),
                "-S".to_string(),
                "firefox".to_string(),
            ],
        };
        assert_eq!(command_display(&spec), "sudo pacman -S firefox");
    }

    #[test]
    fn test_aur_helper_install_command_paru() {
        let config = AppConfig {
            aur_helper: "paru".to_string(),
            ..Default::default()
        };
        let helper = AurHelperCommand::new(&config);
        let cmd = helper.install_command(&["firefox"]);
        assert_eq!(cmd.prog, "paru");
    }

    #[test]
    fn test_aur_helper_install_command_yay() {
        let config = AppConfig {
            aur_helper: "yay".to_string(),
            ..Default::default()
        };
        let helper = AurHelperCommand::new(&config);
        let cmd = helper.install_command(&["vlc"]);
        assert_eq!(cmd.prog, "yay");
    }

    #[test]
    fn test_aur_helper_remove_command() {
        let config = AppConfig::default();
        let helper = AurHelperCommand::new(&config);
        let cmd = helper.remove_command(&["firefox"]);
        assert!(cmd.args.contains(&"-Rns".to_string()));
    }

    #[test]
    fn test_downgrade_command_builder() {
        let cmd = PackageService::build_downgrade_command("firefox", "120.0-1");
        assert_eq!(cmd.prog, "sudo");
        assert!(cmd.args.contains(&"pacman".to_string()));
        assert!(cmd.args.contains(&"-U".to_string()));
        assert!(cmd.args.iter().any(|a| a.contains("archive.archlinux.org")));
    }

    #[test]
    fn test_plan_package_transaction_ecosystems() {
        let config = AppConfig::default();

        // NPM
        let pkg_npm = Package {
            name: "express".to_string(),
            source: PackageSource::Npm,
            is_installed: false,
            ..Default::default()
        };
        let cmds = plan_package_transaction(&[pkg_npm], &config);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].prog, "npm");
        assert_eq!(cmds[0].args, vec!["install", "-g", "express"]);

        // Cargo
        let pkg_cargo = Package {
            name: "ripgrep".to_string(),
            source: PackageSource::Cargo,
            is_installed: false,
            ..Default::default()
        };
        let cmds = plan_package_transaction(&[pkg_cargo], &config);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].prog, "cargo");
        assert_eq!(cmds[0].args, vec!["install", "ripgrep"]);

        // Pip
        let pkg_pip = Package {
            name: "requests".to_string(),
            source: PackageSource::Pip,
            is_installed: false,
            ..Default::default()
        };
        let cmds = plan_package_transaction(&[pkg_pip], &config);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].prog, "pip");
        assert_eq!(cmds[0].args, vec!["install", "--user", "requests"]);

        // Uninstall NPM
        let pkg_npm_un = Package {
            name: "express".to_string(),
            source: PackageSource::Npm,
            is_installed: true,
            ..Default::default()
        };
        let cmds = plan_package_transaction(&[pkg_npm_un], &config);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].prog, "npm");
        assert_eq!(cmds[0].args, vec!["uninstall", "-g", "express"]);
    }
}

// Add a test-only helper to PacmanProvider for unit tests
#[cfg(test)]
impl PacmanProvider {
    pub fn parse_entry_for_test(header: &str, desc: &str) -> Option<Package> {
        Self::parse_entry(header, desc)
    }
}
