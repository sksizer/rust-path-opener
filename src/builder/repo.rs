use std::path::PathBuf;
use worktree::WorktreeBuilder;

pub mod worktree;

pub struct Repo {
    cwd: PathBuf,
}

impl Repo {
    pub fn new(cwd: PathBuf) -> Self {
        Repo { cwd: cwd.clone() }
    }
    pub fn worktree(self) -> WorktreeBuilder {
        WorktreeBuilder::new(self.cwd)
    }
}

pub fn repo(path: impl Into<PathBuf>) -> Repo {
    Repo::new(path.into())
}
