pub mod builder;
pub mod cmd;
pub mod error;
pub mod info;

pub use cmd::GitCommand;
pub use cmd::git;
pub use error::GitError;

#[cfg(test)]
mod test {
    #[test]
    fn test_info_api() {
        let path = crate::info::path();
        let present = crate::info::available();
        let info = crate::info::get();
    }

    // #[test]
    // fn test_builder_git() {
    //     crate::builder::git();
    // }

    #[test]
    fn test_builder_version() {
        crate::builder::version();
    }

    #[test]
    fn test_builder_repo() {
        let temp_dir = std::env::temp_dir();
        let repo = crate::builder::repo(temp_dir);
    }

    #[test]
    fn test_builder_repo_worktree() {
        let temp_dir = std::env::temp_dir();
        let worktree = crate::builder::repo(temp_dir).worktree();
        let add = worktree.add("test".to_string());

        let temp_dir = std::env::temp_dir();
        let worktree = crate::builder::repo(temp_dir)
            .worktree()
            .add("test".to_string());

        let temp_dir = std::env::temp_dir();
        let add = crate::builder::repo(temp_dir)
            .worktree()
            .add("test".to_string());
        let result = add.run();
    }

    // #[test]
    // fn test_builder_worktree() {
    //     let worktree = crate::builder::worktree();
    // }
    //
}
