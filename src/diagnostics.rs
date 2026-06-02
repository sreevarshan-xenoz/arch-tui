//! System diagnostics collection.
//!
//! Gathers system health information: package manager status, disk usage,
//! availability, and other diagnostics using sysinfo for cross-platform support.

use std::process::Command;
use sysinfo::{System, Disks};

#[derive(Debug, Clone)]
pub struct DiagnosticItem {
    pub label: String,
    pub status: String,
}

pub fn run_diagnostics() -> Vec<DiagnosticItem> {
    let mut items = Vec::new();
    
    // Package manager status
    if command_exists("pacman") {
        let aur_helper = if command_exists("paru") {
            "paru"
        } else if command_exists("yay") {
            "yay"
        } else {
            "none"
        };

        items.push(DiagnosticItem {
            label: "pacman binary".to_string(),
            status: "OK".to_string(),
        });
        items.push(DiagnosticItem {
            label: "AUR helper".to_string(),
            status: aur_helper.to_string(),
        });
        items.push(DiagnosticItem {
            label: "pacman db lock".to_string(),
            status: if std::path::Path::new("/var/lib/pacman/db.lck").exists() {
                "LOCKED".to_string()
            } else {
                "clear".to_string()
            },
        });
    } else if command_exists("brew") {
        items.push(DiagnosticItem {
            label: "brew binary".to_string(),
            status: "OK".to_string(),
        });
    } else if command_exists("scoop") {
        items.push(DiagnosticItem {
            label: "scoop binary".to_string(),
            status: "OK".to_string(),
        });
    } else if command_exists("apt") {
        items.push(DiagnosticItem {
            label: "apt binary".to_string(),
            status: "OK".to_string(),
        });
    }

    // Disk space (Root or C:)
    let disks = Disks::new_with_refreshed_list();
    let root_disk = if cfg!(target_os = "windows") {
        disks.iter().find(|d| d.mount_point().to_str() == Some("C:\\"))
    } else {
        disks.iter().find(|d| d.mount_point().to_str() == Some("/"))
    };

    if let Some(disk) = root_disk {
        let used = disk.total_space() - disk.available_space();
        let usage_pct = (used as f64 / disk.total_space() as f64) * 100.0;
        items.push(DiagnosticItem {
            label: "disk usage".to_string(),
            status: format!("{:.1}% used", usage_pct),
        });
    }

    items
}

pub fn get_system_info() -> Vec<DiagnosticItem> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let mut items = vec![
        DiagnosticItem {
            label: "OS".to_string(),
            status: System::name().unwrap_or_else(|| "unknown".to_string()),
        },
        DiagnosticItem {
            label: "OS Version".to_string(),
            status: System::os_version().unwrap_or_else(|| "unknown".to_string()),
        },
        DiagnosticItem {
            label: "Kernel".to_string(),
            status: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
        },
        DiagnosticItem {
            label: "Hostname".to_string(),
            status: System::host_name().unwrap_or_else(|| "unknown".to_string()),
        },
        DiagnosticItem {
            label: "Uptime".to_string(),
            status: format_uptime(System::uptime()),
        },
        DiagnosticItem {
            label: "CPU".to_string(),
            status: sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_else(|| "unknown".to_string()),
        },
        DiagnosticItem {
            label: "CPU Cores".to_string(),
            status: sys.cpus().len().to_string(),
        },
        DiagnosticItem {
            label: "Memory".to_string(),
            status: format!(
                "{:.1}GB / {:.1}GB used",
                (sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0),
                (sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0)
            ),
        },
    ];

    // Installed packages count
    if command_exists("pacman") {
        if let Ok(count) = get_total_packages_pacman() {
            items.push(DiagnosticItem {
                label: "Installed packages".to_string(),
                status: count,
            });
        }
    } else if command_exists("brew") {
        if let Ok(count) = get_total_packages_brew() {
            items.push(DiagnosticItem {
                label: "Brew packages".to_string(),
                status: count,
            });
        }
    }

    items
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    if days > 0 {
        format!("{}d {}h {}m", days, hours, minutes)
    } else {
        format!("{}h {}m", hours, minutes)
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

fn get_total_packages_pacman() -> Result<String, std::io::Error> {
    let output = Command::new("pacman").arg("-Qq").output()?;
    let count = String::from_utf8_lossy(&output.stdout).lines().count();
    Ok(count.to_string())
}

fn get_total_packages_brew() -> Result<String, std::io::Error> {
    let output = Command::new("brew").arg("list").output()?;
    let count = String::from_utf8_lossy(&output.stdout).lines().count();
    Ok(count.to_string())
}

#[derive(Debug, Clone)]
pub struct OrphanPackage {
    pub name: String,
}

pub fn find_orphan_packages() -> Vec<OrphanPackage> {
    if !command_exists("pacman") {
        return Vec::new();
    }

    let mut orphans = Vec::new();
    let explicit_output = Command::new("pacman")
        .args(["-Qet", "--color", "never"])
        .output();

    if let Ok(output) = explicit_output {
        if output.status.success() {
            let packages = String::from_utf8_lossy(&output.stdout);
            for line in packages.lines() {
                let pkg_name = line.split_whitespace().next().unwrap_or("");
                if !pkg_name.is_empty() {
                    orphans.push(OrphanPackage {
                        name: pkg_name.to_string(),
                    });
                }
            }
        }
    }
    orphans
}

#[derive(Debug, Clone)]
pub struct PackageSize {
    pub name: String,
    pub size_kb: u64,
    pub size_formatted: String,
}

pub fn get_package_sizes() -> Vec<PackageSize> {
    if !command_exists("pacman") {
        return Vec::new();
    }

    let mut packages = Vec::new();
    let output = Command::new("pacman")
        .args(["-Qi", "--color", "never"])
        .output();

    if let Ok(output) = output {
        let content = String::from_utf8_lossy(&output.stdout);
        let mut current_pkg = String::new();

        for line in content.lines() {
            if line.starts_with("Name            :") {
                if let Some(name) = line.split(':').nth(1) {
                    current_pkg = name.trim().to_string();
                }
            } else if line.starts_with("Installed Size  :") {
                if let Some(size_str) = line.split(':').nth(1) {
                    let size_str = size_str.trim();
                    let size_val: f64 = size_str
                        .split_whitespace()
                        .next()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0.0);
                    let unit = size_str.split_whitespace().nth(1).unwrap_or("");

                    let size_kb = match unit {
                        "KiB" => size_val as u64,
                        "MiB" => (size_val * 1024.0) as u64,
                        "GiB" => (size_val * 1024.0 * 1024.0) as u64,
                        _ => size_val as u64,
                    };

                    if !current_pkg.is_empty() {
                        packages.push(PackageSize {
                            name: current_pkg.clone(),
                            size_kb,
                            size_formatted: size_str.to_string(),
                        });
                    }
                }
            }
        }
    }

    packages.sort_by(|a, b| b.size_kb.cmp(&a.size_kb));
    packages
}

#[derive(Debug, Clone)]
pub struct CacheInfo {
    pub path: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub file_count: usize,
}

pub fn get_cache_info() -> Vec<CacheInfo> {
    let mut caches = Vec::new();

    if command_exists("pacman") {
        let pacman_cache = "/var/cache/pacman/pkg";
        if let Ok(info) = get_dir_size(pacman_cache) {
            caches.push(info);
        }
    }

    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        let aur_caches = [
            format!("{}/.cache/paru", home),
            format!("{}/.cache/yay", home),
        ];
        for cache_path in aur_caches {
            if std::path::Path::new(&cache_path).exists() {
                if let Ok(info) = get_dir_size(&cache_path) {
                    caches.push(info);
                }
            }
        }
    }

    caches
}

fn get_dir_size(path: &str) -> Result<CacheInfo, std::io::Error> {
    let mut total_size = 0u64;
    let mut file_count = 0usize;

    let entries = std::fs::read_dir(path)?;
    for entry in entries.flatten() {
        if let Ok(metadata) = entry.metadata() {
            if metadata.is_file() {
                total_size += metadata.len();
                file_count += 1;
            } else if metadata.is_dir() {
                if let Ok(sub_info) = get_dir_size(&entry.path().to_string_lossy()) {
                    total_size += sub_info.size_bytes;
                    file_count += sub_info.file_count;
                }
            }
        }
    }

    let size_formatted = format_size(total_size);
    Ok(CacheInfo {
        path: path.to_string(),
        size_bytes: total_size,
        size_formatted,
        file_count,
    })
}

fn format_size(bytes: u64) -> String {
    let kb = bytes as f64 / 1024.0;
    let mb = kb / 1024.0;
    let gb = mb / 1024.0;

    if gb >= 1.0 {
        format!("{:.2} GB", gb)
    } else if mb >= 1.0 {
        format!("{:.2} MB", mb)
    } else {
        format!("{:.2} KB", kb)
    }
}

#[derive(Debug, Clone)]
pub struct ForeignPackage {
    pub name: String,
    pub version: String,
    pub source: String,
}

pub fn get_foreign_packages() -> Vec<ForeignPackage> {
    if !command_exists("pacman") {
        return Vec::new();
    }

    let mut packages = Vec::new();
    let output = Command::new("pacman")
        .args(["-Qmq", "--color", "never"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let content = String::from_utf8_lossy(&output.stdout);
            for line in content.lines() {
                let pkg_name = line.trim();
                if !pkg_name.is_empty() {
                    if let Ok(info) = get_package_info_pacman(pkg_name) {
                        packages.push(ForeignPackage {
                            name: pkg_name.to_string(),
                            version: info.0,
                            source: info.1,
                        });
                    }
                }
            }
        }
    }
    packages
}

fn get_package_info_pacman(pkg_name: &str) -> Result<(String, String), std::io::Error> {
    let output = Command::new("pacman").args(["-Qi", pkg_name]).output()?;
    if !output.status.success() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Package not found"));
    }

    let content = String::from_utf8_lossy(&output.stdout);
    let mut version = String::new();
    let mut source = String::from("AUR");

    for line in content.lines() {
        if line.starts_with("Version        :") {
            version = line.split(':').nth(1).unwrap_or("").trim().to_string();
        } else if line.starts_with("Repository    :") {
            source = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
    }
    Ok((version, source))
}

pub fn get_repository_packages_count() -> usize {
    if !command_exists("pacman") {
        return 0;
    }
    let output = Command::new("pacman")
        .args(["-Qq", "--color", "never"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let total = String::from_utf8_lossy(&output.stdout).lines().count();
            let foreign = get_foreign_packages().len();
            return total.saturating_sub(foreign);
        }
    }
    0
}

#[derive(Debug, Clone)]
pub struct PackageGroup {
    pub name: String,
    pub member_count: usize,
}

pub fn get_package_groups() -> Vec<PackageGroup> {
    if !command_exists("pacman") {
        return Vec::new();
    }
    let mut groups = Vec::new();
    let output = Command::new("pacman")
        .args(["-Sg", "--color", "never"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let content = String::from_utf8_lossy(&output.stdout);
            for line in content.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    groups.push(PackageGroup {
                        name: parts[0].to_string(),
                        member_count: parts.len() - 1,
                    });
                }
            }
        }
    }
    groups.sort_by(|a, b| b.member_count.cmp(&a.member_count));
    groups
}
