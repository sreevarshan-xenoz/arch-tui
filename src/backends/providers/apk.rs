use async_trait::async_trait;
use crate::errors::Result;
use crate::models::{OutdatedPackage, Package};
use crate::traits::{PackageProvider, UpdateProvider};

pub struct ApkProvider;
impl ApkProvider { pub fn new() -> Self { Self } }

#[async_trait]
impl PackageProvider for ApkProvider {
    async fn search(&self, _query: &str) -> Result<Vec<Package>> { Ok(Vec::new()) }
    async fn is_installed(&self, _pkg_name: &str) -> bool { false }
}
#[async_trait]
impl UpdateProvider for ApkProvider {
    async fn check_updates(&self) -> Result<usize> { Ok(0) }
    async fn get_outdated_packages(&self) -> Result<Vec<OutdatedPackage>> { Ok(Vec::new()) }
}
