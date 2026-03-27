use crate::GitError;
use crate::builder::version;
use crate::builder::version::Version;
use std::path::PathBuf;
use which::which;

pub fn available() -> bool {
    which("git").is_ok()
}

pub fn path() -> Result<PathBuf, GitError> {
    match which("git") {
        Ok(path) => Ok(path),
        Err(_) => Err(GitError::NotAvailable),
    }
}
pub struct GitInfo {
    pub path: PathBuf,
    pub version: Version,
}

pub fn get() -> Result<GitInfo, GitError> {
    let path = path();
    match path {
        Ok(path) => {
            let version = version::version();
            match version {
                Ok(version) => Ok(GitInfo { path, version }),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}
