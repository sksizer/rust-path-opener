use crate::GitError;

pub fn version() -> Result<Version, GitError> {
    let cmd = crate::git().arg("--version").run();

    match cmd {
        Ok(output) => Ok(parse_git_version(output)),
        Err(e) => match e {
            GitError::Exec(_) => Err(GitError::NotAvailable),
            other => Err(other),
        },
    }
}

pub struct Version {
    pub number: String,
    pub platform: String,
}

pub fn parse_git_version(version_str: String) -> Version {
    let raw = version_str.trim();

    // "git version 2.50.1 (Apple Git-155)" or "git version 2.50.1"
    let raw = raw.trim().strip_prefix("git version ").unwrap();

    let mut parts = raw.splitn(2, ' ');
    let version = parts.next().unwrap();
    let platform = parts.next().unwrap_or("unknown");

    Version {
        number: version.to_string(),
        platform: platform.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_api() {
        let v = crate::builder::version::version();
    }
}
