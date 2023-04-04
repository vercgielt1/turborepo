use std::{
    borrow::Cow,
    ffi::OsStr,
    fmt,
    path::{Components, Path, PathBuf},
};

use serde::Serialize;

use crate::{AnchoredSystemPathBuf, IntoSystem, PathValidationError, RelativeSystemPathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize)]
pub struct AbsoluteSystemPathBuf(PathBuf);

impl AbsoluteSystemPathBuf {
    /// Create a new AbsoluteSystemPathBuf from `unchecked_path`.
    /// Confirms that `unchecked_path` is absolute and converts it to a system
    /// path.
    ///
    /// # Arguments
    ///
    /// * `unchecked_path`: The path to be validated and converted to an
    ///   `AbsoluteSystemPathBuf`.
    ///
    /// returns: Result<AbsoluteSystemPathBuf, PathValidationError>
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::{Path, PathBuf};
    /// use turbopath::AbsoluteSystemPathBuf;
    /// let path = PathBuf::from("/Users/user");
    /// let absolute_path = AbsoluteSystemPathBuf::new(path).unwrap();
    /// #[cfg(windows)]
    /// assert_eq!(absolute_path.as_path(), Path::new("\\Users\\user"));
    /// assert_eq!(absolute_path.as_path(), Path::new("/Users/user"));
    /// ```
    pub fn new(unchecked_path: impl Into<PathBuf>) -> Result<Self, PathValidationError> {
        let unchecked_path = unchecked_path.into();
        if !unchecked_path.is_absolute() {
            return Err(PathValidationError::NotAbsolute(unchecked_path));
        }

        let system_path = unchecked_path.into_system()?;
        Ok(AbsoluteSystemPathBuf(system_path))
    }

    /// Converts `path` to an `AbsoluteSystemPathBuf` without validating that
    /// it is either absolute or a system path.
    ///
    /// # Arguments
    ///
    /// * `path`: The path to be converted to an `AbsoluteSystemPathBuf`.
    ///
    /// returns: AbsoluteSystemPathBuf
    pub fn new_unchecked(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        AbsoluteSystemPathBuf(path)
    }

    /// Anchors `path` at `self`.
    ///
    /// # Arguments
    ///
    /// * `path`: The path to be anchored at `self`
    ///
    /// returns: Result<AnchoredSystemPathBuf, PathValidationError>
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// use turbopath::{AbsoluteSystemPathBuf, AnchoredSystemPathBuf};
    /// let base = AbsoluteSystemPathBuf::new("/Users/user").unwrap();
    /// let anchored_path = AbsoluteSystemPathBuf::new("/Users/user/Documents").unwrap();
    /// let anchored_path = base.anchor(&anchored_path).unwrap();
    /// assert_eq!(anchored_path.as_path(), Path::new("Documents"));
    /// ```
    pub fn anchor(
        &self,
        path: &AbsoluteSystemPathBuf,
    ) -> Result<AnchoredSystemPathBuf, PathValidationError> {
        AnchoredSystemPathBuf::strip_root(self, path)
    }

    /// Resolves `path` with `self` as anchor.
    ///
    /// # Arguments
    ///
    /// * `path`: The path to be anchored at `self`
    ///
    /// returns: AbsoluteSystemPathBuf
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// use turbopath::{AbsoluteSystemPathBuf, AnchoredSystemPathBuf};
    /// let absolute_path = AbsoluteSystemPathBuf::new("/Users/user").unwrap();
    /// let anchored_path = AnchoredSystemPathBuf::new_unchecked("Documents");
    /// let resolved_path = absolute_path.resolve(&anchored_path);
    /// #[cfg(windows)]
    /// assert_eq!(resolved_path.as_path(), Path::new("\\Users\\user\\Documents"));
    /// assert_eq!(resolved_path.as_path(), Path::new("/Users/user/Documents"));
    /// ```
    pub fn resolve(&self, path: &AnchoredSystemPathBuf) -> AbsoluteSystemPathBuf {
        AbsoluteSystemPathBuf(self.0.join(path.as_path()))
    }

    pub fn as_path(&self) -> &Path {
        self.0.as_path()
    }

    pub fn components(&self) -> Components<'_> {
        self.0.components()
    }

    pub fn parent(&self) -> Option<Self> {
        self.0
            .parent()
            .map(|p| AbsoluteSystemPathBuf(p.to_path_buf()))
    }

    pub fn starts_with<P: AsRef<Path>>(&self, base: P) -> bool {
        self.0.starts_with(base.as_ref())
    }

    pub fn ends_with<P: AsRef<Path>>(&self, child: P) -> bool {
        self.0.ends_with(child.as_ref())
    }

    pub fn join<P: AsRef<Path>>(&self, path: P) -> AbsoluteSystemPathBuf {
        AbsoluteSystemPathBuf(self.0.join(path))
    }

    pub fn join_relative(&self, path: RelativeSystemPathBuf) -> AbsoluteSystemPathBuf {
        AbsoluteSystemPathBuf(self.0.join(path.as_path()))
    }

    pub fn to_str(&self) -> Result<&str, PathValidationError> {
        self.0.to_str().ok_or(PathValidationError::InvalidUnicode)
    }

    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        self.0.to_string_lossy()
    }

    pub fn file_name(&self) -> Option<&OsStr> {
        self.0.file_name()
    }

    pub fn exists(&self) -> bool {
        self.0.exists()
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }

    pub fn extension(&self) -> Option<&OsStr> {
        self.0.extension()
    }
}

impl fmt::Display for AbsoluteSystemPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(f)
    }
}
