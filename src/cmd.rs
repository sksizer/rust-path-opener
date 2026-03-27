//! Low-level git command builder.
//!
//! Every git interaction in this crate bottlenecks through [`GitCommand`],
//! which handles the repeated pattern of:
//!
//! 1. Spawn `git <args>` in a working directory
//! 2. Capture stdout + stderr
//! 3. Check exit status
//! 4. Return typed result or error
//!
//! Three execution modes cover every use case:
//!
//! - [`run`](GitCommand::run) — stdout on success, error on failure
//! - [`run_raw`](GitCommand::run_raw) — full `Output` for stderr inspection
//! - [`check`](GitCommand::check) — boolean probe (exit 0 = true)

use std::path::PathBuf;
use std::process::{Command, Output};

use crate::GitError;

/// Entry point. `git(path).args(&["status"]).run()`
pub fn git() -> GitCommand {
    GitCommand::new()
}

/// A builder for a single git subprocess invocation.
pub struct GitCommand {
    args: Vec<String>,
    cwd: Option<PathBuf>,
}

impl Default for GitCommand {
    fn default() -> Self {
        Self::new()
    }
}
impl GitCommand {
    pub fn new() -> Self {
        Self {
            args: Vec::new(),
            cwd: None,
        }
    }

    pub fn dir(&mut self, cwd: impl Into<PathBuf>) -> &mut Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Append a single argument.
    pub fn arg(&mut self, arg: impl AsRef<str>) -> &mut Self {
        self.args.push(arg.as_ref().to_string());
        self
    }

    /// Append multiple arguments.
    pub fn args(&mut self, args: &[&str]) -> &mut Self {
        self.args.extend(args.iter().map(|s| s.to_string()));
        self
    }

    /// Run the command. Returns trimmed stdout on success, GitError on failure.
    pub fn run(&self) -> Result<String, GitError> {
        let output = self.run_raw()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(GitError::Command {
                command: self.label(),
                stderr,
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run the command and return the raw `Output`.
    ///
    /// Use this when you need to inspect stderr or handle non-zero exits yourself.
    pub fn run_raw(&self) -> Result<Output, GitError> {
        let mut cmd = Command::new("git");
        cmd.args(&self.args);

        if let Some(cwd) = &self.cwd {
            cmd.current_dir(cwd);
        }

        cmd.output().map_err(GitError::Exec)
    }

    /// Run the command as a boolean probe.
    ///
    /// Returns `Ok(true)` on exit 0, `Ok(false)` on non-zero.
    /// Only returns `Err` if the process failed to spawn.
    pub fn check(&self) -> Result<bool, GitError> {
        let output = self.run_raw()?;
        Ok(output.status.success())
    }

    /// Human-readable label for error messages.
    fn label(&self) -> String {
        format!("git {}", self.args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::GitCommand;

    #[test]
    fn test_api() {
        let mut cmd = GitCommand::new();
        cmd.arg("--version");
        let result = cmd.run().unwrap();
        assert!(result.contains("git version"));
    }
}
