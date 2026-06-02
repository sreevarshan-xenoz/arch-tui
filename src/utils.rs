//! Utility functions for secure operations
//!
//! This module provides secure password handling and other security-related utilities.

use secrecy::{ExposeSecret, SecretString};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Circuit breaker for external API calls to prevent flooding when service is down
pub struct CircuitBreaker {
    failure_count: AtomicU32,
    last_failure: AtomicU64,
    state: AtomicU32,
}

impl CircuitBreaker {
    pub const FAILURE_THRESHOLD: u32 = 5;
    pub const RECOVERY_SECS: u64 = 30;
    pub const STATE_CLOSED: u32 = 0;
    pub const STATE_OPEN: u32 = 1;
    pub const STATE_HALF_OPEN: u32 = 2;

    pub fn new() -> Self {
        Self {
            failure_count: AtomicU32::new(0),
            last_failure: AtomicU64::new(0),
            state: AtomicU32::new(Self::STATE_CLOSED),
        }
    }

    pub fn is_available(&self) -> bool {
        let state = self.state.load(Ordering::SeqCst);
        match state {
            Self::STATE_CLOSED => true,
            Self::STATE_OPEN => {
                let last_fail = self.last_failure.load(Ordering::SeqCst);
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let elapsed = now.saturating_sub(last_fail);
                
                if elapsed > Self::RECOVERY_SECS {
                    self.state.store(Self::STATE_HALF_OPEN, Ordering::SeqCst);
                    true
                } else {
                    false
                }
            }
            Self::STATE_HALF_OPEN => true,
            _ => false,
        }
    }

    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
        self.state.store(Self::STATE_CLOSED, Ordering::SeqCst);
    }

    pub fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_failure.store(now, Ordering::SeqCst);

        if count >= Self::FAILURE_THRESHOLD {
            self.state.store(Self::STATE_OPEN, Ordering::SeqCst);
            tracing::warn!(
                "Circuit breaker opened due to {} consecutive failures",
                count
            );
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Securely verify sudo password
pub fn check_sudo_password(password: &SecretString) -> bool {
    let mut child = match Command::new("sudo")
        .arg("-S")
        .arg("-v")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            tracing::error!("Failed to spawn sudo process: {}", e);
            return false;
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = writeln!(stdin, "{}", password.expose_secret()) {
            tracing::error!("Failed to write password to stdin: {}", e);
            return false;
        }
    }

    match child.wait() {
        Ok(status) => {
            let success = status.success();
            if !success {
                tracing::warn!("Sudo authentication failed");
            }
            success
        }
        Err(e) => {
            tracing::error!("Failed to wait for sudo process: {}", e);
            false
        }
    }
}

/// Secure password input with masking
pub struct PasswordInput {
    value: SecretString,
}

impl PasswordInput {
    pub fn new() -> Self {
        Self {
            value: SecretString::new(String::new()),
        }
    }

    pub fn from_string(s: String) -> Self {
        Self {
            value: SecretString::new(s),
        }
    }

    pub fn push(&mut self, c: char) {
        let mut current = self.value.expose_secret().to_string();
        current.push(c);
        self.value = SecretString::new(current);
    }

    pub fn pop(&mut self) {
        let mut current = self.value.expose_secret().to_string();
        current.pop();
        self.value = SecretString::new(current);
    }

    pub fn len(&self) -> usize {
        self.value.expose_secret().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        self.value = SecretString::new(String::new());
    }

    pub fn get_secret(&self) -> &SecretString {
        &self.value
    }

    pub fn expose_secret(&self) -> &str {
        self.value.expose_secret()
    }

    /// Get masked representation for display
    pub fn masked(&self) -> String {
        format!("{}█", "*".repeat(self.len()))
    }
}

impl Default for PasswordInput {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate that a path doesn't contain path traversal attempts
pub fn validate_path(path: &std::path::Path) -> bool {
    use std::path::Component;
    let components = path.components();
    for comp in components {
        match comp {
            Component::ParentDir => return false,
            Component::Normal(s) => {
                let s_str = s.to_string_lossy();
                if s_str.starts_with('.')
                    && s_str != ".config"
                    && s_str != ".local"
                    && s_str != ".cache"
                    && !s_str.starts_with(".tmp")
                {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_input() {
        let mut pwd = PasswordInput::new();
        assert!(pwd.is_empty());

        pwd.push('a');
        pwd.push('b');
        pwd.push('c');

        assert_eq!(pwd.len(), 3);
        assert_eq!(pwd.expose_secret(), "abc");
        assert_eq!(pwd.masked(), "***█");

        pwd.pop();
        assert_eq!(pwd.len(), 2);
        assert_eq!(pwd.expose_secret(), "ab");

        pwd.clear();
        assert!(pwd.is_empty());
    }

    #[test]
    fn test_password_input_from_string() {
        let pwd = PasswordInput::from_string("test123".to_string());
        assert_eq!(pwd.len(), 7);
        assert_eq!(pwd.masked(), "*******█");
    }
}
