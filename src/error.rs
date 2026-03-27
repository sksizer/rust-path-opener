use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git not available")]
    NotAvailable,

    #[error("failed to execute git: {0}")]
    Exec(#[from] std::io::Error),

    #[error("git {command} failed: {stderr}")]
    Command { command: String, stderr: String },

    #[error("not a git repository: {}", .0.display())]
    NotARepo(PathBuf),
}
