mod absolute_system_path;
mod absolute_system_path_buf;
mod anchored_system_path_buf;
mod relative_system_path;
mod relative_system_path_buf;
mod relative_unix_path_buf;

use std::path::{Path, PathBuf};

pub use absolute_system_path::AbsoluteSystemPath;
pub use absolute_system_path_buf::AbsoluteSystemPathBuf;
pub use anchored_system_path_buf::AnchoredSystemPathBuf;
use path_slash::{PathBufExt, PathExt};
pub use relative_system_path::RelativeSystemPath;
pub use relative_system_path_buf::RelativeSystemPathBuf;
pub use relative_unix_path_buf::RelativeUnixPathBuf;
use thiserror::Error;

// Custom error type for path validation errors
#[derive(Debug, Error)]
pub enum PathValidationError {
    #[error("Path is non-UTF-8")]
    NonUtf8,
    #[error("Path is not absolute: {0}")]
    NotAbsolute(PathBuf),
    #[error("Path is not relative: {0}")]
    NotRelative(PathBuf),
    #[error("Path {0} is not parent of {1}")]
    NotParent(String, String),
}

trait IntoSystem {
    fn into_system(&self) -> Result<PathBuf, PathValidationError>;
}

trait IntoUnix {
    fn into_unix(&self) -> Result<PathBuf, PathValidationError>;
}

impl IntoSystem for Path {
    fn into_system(&self) -> Result<PathBuf, PathValidationError> {
        let path_str = self.to_str().ok_or(PathValidationError::NonUtf8)?;

        Ok(PathBuf::from_slash(path_str))
    }
}

impl IntoUnix for Path {
    /// NOTE: `into_unix` *only* converts Windows paths to Unix paths *on* a
    /// Windows system. Do not pass a Windows path on a Unix system and
    /// assume it'll be converted.
    fn into_unix(&self) -> Result<PathBuf, PathValidationError> {
        Ok(PathBuf::from(
            self.to_slash()
                .ok_or(PathValidationError::NonUtf8)?
                .as_ref(),
        ))
    }
}
