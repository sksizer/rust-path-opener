mod add;
mod remove;

use std::path::PathBuf;

use crate::GitError;
use crate::cmd::git;

pub struct WorktreeBuilder {
    cwd: PathBuf,
}

impl WorktreeBuilder {
    pub(in crate::builder) fn new(cwd: PathBuf) -> Self {
        WorktreeBuilder { cwd }
    }

    /// `git worktree add <path> [options]`
    pub fn add(self, path: impl Into<PathBuf>) -> Add {
        Add {
            cwd: self.cwd,
            path: path.into(),
            branch_mode: BranchMode::None,
            detach: false,
            no_checkout: false,
            force: false,
        }
    }

    /// `git worktree remove [--force] <path>`
    pub fn remove(self, path: impl Into<PathBuf>) -> Remove {
        Remove {
            cwd: self.cwd,
            path: path.into(),
            force: false,
        }
    }
}

// ── Add ──────────────────────────────────────────────────────────────

/// How to handle branch selection when adding a worktree.
enum BranchMode {
    /// No branch specified — git creates one from the directory name.
    None,
    /// `-b <name>` — create a new branch.
    New(String),
    /// Positional `<branch>` — checkout an existing branch.
    Existing(String),
}

pub struct Add {
    cwd: PathBuf,
    path: PathBuf,
    branch_mode: BranchMode,
    detach: bool,
    no_checkout: bool,
    force: bool,
}

impl Add {
    /// Checkout an existing branch into the worktree.
    ///
    /// Produces: `git worktree add <path> <branch>`
    pub fn branch(&mut self, name: impl Into<String>) -> &mut Self {
        self.branch_mode = BranchMode::Existing(name.into());
        self
    }

    /// Create a new branch and check it out in the worktree.
    ///
    /// Produces: `git worktree add -b <name> <path>`
    pub fn new_branch(&mut self, name: impl Into<String>) -> &mut Self {
        self.branch_mode = BranchMode::New(name.into());
        self
    }

    pub fn detach(&mut self) -> &mut Self {
        self.detach = true;
        self
    }

    pub fn no_checkout(&mut self) -> &mut Self {
        self.no_checkout = true;
        self
    }

    pub fn force(&mut self) -> &mut Self {
        self.force = true;
        self
    }

    pub fn run(&self) -> Result<(), GitError> {
        let mut cmd = crate::git();
        cmd.dir(&self.cwd);
        cmd.args(&["worktree", "add"]);

        if self.force {
            cmd.arg("--force");
        }
        if self.detach {
            cmd.arg("--detach");
        }
        if self.no_checkout {
            cmd.arg("--no-checkout");
        }
        if let BranchMode::New(ref name) = self.branch_mode {
            cmd.arg("-b").arg(name);
        }

        // Path is always required
        cmd.arg(self.path.to_string_lossy().as_ref());

        // Existing branch goes after the path as a positional arg
        if let BranchMode::Existing(ref name) = self.branch_mode {
            cmd.arg(name);
        }

        cmd.run()?;
        Ok(())
    }
}

// ── Remove ───────────────────────────────────────────────────────────

pub struct Remove {
    cwd: PathBuf,
    path: PathBuf,
    force: bool,
}

impl Remove {
    pub fn force(&mut self) -> &mut Self {
        self.force = true;
        self
    }

    pub fn run(&self) -> Result<(), GitError> {
        let mut git = git();
        let cmd = git.dir(&self.cwd);
        cmd.args(&["worktree", "remove"]);

        if self.force {
            cmd.arg("--force");
        }

        cmd.arg(self.path.to_string_lossy().as_ref());

        cmd.run()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::builder::repo;

    #[test]
    fn test_add_new_branch_compiles() {
        let mut add = repo("/repo").worktree().add("/tmp/my-worktree");
        add.new_branch("my-feature").no_checkout();
    }

    #[test]
    fn test_add_existing_branch_compiles() {
        let mut add = repo("/repo").worktree().add("/tmp/my-worktree");
        add.branch("existing-feature");
    }

    #[test]
    fn test_remove_compiles() {
        let mut rm = repo("/repo").worktree().remove("/tmp/my-worktree");
        rm.force();
    }
}
