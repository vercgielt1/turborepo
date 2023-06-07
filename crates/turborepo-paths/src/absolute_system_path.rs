#[cfg(not(windows))]
use std::os::unix::fs::symlink as symlink_file;
#[cfg(not(windows))]
use std::os::unix::fs::symlink as symlink_dir;
#[cfg(windows)]
use std::os::windows::fs::{symlink_dir, symlink_file};
use std::{
    borrow::Cow,
    fmt, fs,
    fs::Metadata,
    io,
    path::{Component, Components, Path, PathBuf},
};

use path_clean::PathClean;
use path_slash::CowExt;

use crate::{
    AbsoluteSystemPathBuf, AnchoredSystemPathBuf, IntoSystem, PathError, RelativeUnixPath,
};

pub struct AbsoluteSystemPath(Path);

impl ToOwned for AbsoluteSystemPath {
    type Owned = AbsoluteSystemPathBuf;

    fn to_owned(&self) -> Self::Owned {
        AbsoluteSystemPathBuf(self.0.to_owned())
    }
}

impl AsRef<AbsoluteSystemPath> for AbsoluteSystemPath {
    fn as_ref(&self) -> &AbsoluteSystemPath {
        self
    }
}

impl fmt::Display for AbsoluteSystemPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(f)
    }
}

impl AsRef<Path> for AbsoluteSystemPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AbsoluteSystemPath {
    /// Creates a path that is known to be absolute and a system path.
    /// If either of these conditions are not met, we error.
    /// Does *not* do automatic conversion like `AbsoluteSystemPathBuf::new`
    /// does
    ///
    /// # Arguments
    ///
    /// * `value`: The path to convert to an absolute system path
    ///
    /// returns: Result<&AbsoluteSystemPath, PathError>
    ///
    /// # Examples
    ///
    /// ```
    /// use turbopath::AbsoluteSystemPath;
    /// #[cfg(unix)]
    /// {
    ///   assert!(AbsoluteSystemPath::new("/foo/bar").is_ok());
    ///   assert!(AbsoluteSystemPath::new("foo/bar").is_err());
    ///   assert!(AbsoluteSystemPath::new("C:\\foo\\bar").is_err());
    /// }
    ///
    /// #[cfg(windows)]
    /// {
    ///   assert!(AbsoluteSystemPath::new("C:\\foo\\bar").is_ok());
    ///   assert!(AbsoluteSystemPath::new("foo\\bar").is_err());
    ///   assert!(AbsoluteSystemPath::new("/foo/bar").is_err());
    /// }
    /// ```
    pub fn new<P: AsRef<Path> + ?Sized>(value: &P) -> Result<&Self, PathError> {
        let path = value.as_ref();
        if path.is_relative() {
            return Err(PathError::NotAbsolute(path.to_owned()));
        }
        let path_str = path
            .to_str()
            .ok_or_else(|| PathError::InvalidUnicode(path.to_string_lossy().to_string()))?;

        let system_path = Cow::from_slash(path_str);

        match system_path {
            Cow::Owned(path) => Err(PathError::NotSystem(path.to_string_lossy().to_string())),
            Cow::Borrowed(path) => {
                let path = Path::new(path);
                // copied from stdlib path.rs: relies on the representation of
                // AbsoluteSystemPath being just a Path, the same way Path relies on
                // just being an OsStr
                let absolute_system_path = unsafe { &*(path as *const Path as *const Self) };
                Ok(absolute_system_path)
            }
        }
    }

    pub(crate) fn new_unchecked(path: &Path) -> &Self {
        unsafe { &*(path as *const Path as *const Self) }
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn ancestors(&self) -> impl Iterator<Item = &AbsoluteSystemPath> {
        self.0.ancestors().map(Self::new_unchecked)
    }

    // intended for joining literals or obviously single-token strings
    pub fn join_component(&self, segment: &str) -> AbsoluteSystemPathBuf {
        debug_assert!(!segment.contains(std::path::MAIN_SEPARATOR));
        AbsoluteSystemPathBuf(self.0.join(segment).clean())
    }

    // intended for joining a path composed of literals
    pub fn join_components(&self, segments: &[&str]) -> AbsoluteSystemPathBuf {
        debug_assert!(!segments
            .iter()
            .any(|segment| segment.contains(std::path::MAIN_SEPARATOR)));
        AbsoluteSystemPathBuf(
            self.0
                .join(segments.join(std::path::MAIN_SEPARATOR_STR))
                .clean(),
        )
    }

    pub fn join_unix_path(
        &self,
        unix_path: impl AsRef<RelativeUnixPath>,
    ) -> Result<AbsoluteSystemPathBuf, PathError> {
        let tail = unix_path.as_ref().to_system_path_buf()?;
        Ok(AbsoluteSystemPathBuf(self.0.join(tail.as_path()).clean()))
    }

    pub fn anchor(&self, path: &AbsoluteSystemPath) -> Result<AnchoredSystemPathBuf, PathError> {
        AnchoredSystemPathBuf::new(self, path)
    }

    pub fn ensure_dir(&self) -> Result<(), io::Error> {
        if let Some(parent) = self.0.parent() {
            fs::create_dir_all(parent)
        } else {
            Ok(())
        }
    }

    pub fn symlink_to_file<P: AsRef<Path>>(&self, to: P) -> Result<(), PathError> {
        let system_path = to.as_ref();
        let system_path = system_path.into_system()?;
        symlink_file(system_path, &self.0)?;
        Ok(())
    }

    pub fn symlink_to_dir<P: AsRef<Path>>(&self, to: P) -> Result<(), PathError> {
        let system_path = to.as_ref();
        let system_path = system_path.into_system()?;
        symlink_dir(system_path, &self.0)?;
        Ok(())
    }

    pub fn resolve(&self, path: &AnchoredSystemPathBuf) -> AbsoluteSystemPathBuf {
        let path = self.0.join(path.as_path());
        AbsoluteSystemPathBuf(path)
    }

    // note that this is *not* lstat. If this is a symlink, it
    // will return metadata for the target.
    pub fn stat(&self) -> Result<Metadata, PathError> {
        Ok(fs::metadata(&self.0)?)
    }

    // The equivalent of lstat. Returns the metadata for this file,
    // even if it is a symlink
    pub fn symlink_metadata(&self) -> Result<Metadata, PathError> {
        Ok(fs::symlink_metadata(&self.0)?)
    }

    pub fn read_link(&self) -> Result<PathBuf, io::Error> {
        fs::read_link(&self.0)
    }

    pub fn remove_file(&self) -> Result<(), io::Error> {
        fs::remove_file(&self.0)
    }

    pub fn components(&self) -> Components<'_> {
        self.0.components()
    }

    // Produces a path from self to other, which may include directory traversal
    // tokens. Given that both parameters are absolute, we _should_ always be
    // able to produce such a path. The exception is when crossing drive letters
    // on Windows, where no such path is possible. Since a repository is
    // expected to only reside on a single drive, this shouldn't be an issue.
    pub fn relative_path_to(&self, other: &Self) -> AnchoredSystemPathBuf {
        // Filter the implicit "RootDir" component that exists for unix paths.
        // For windows paths, we may want an assertion that we aren't crossing drives
        let these_components = self
            .components()
            .skip_while(|c| *c == Component::RootDir)
            .collect::<Vec<_>>();
        let other_components = other
            .components()
            .skip_while(|c| *c == Component::RootDir)
            .collect::<Vec<_>>();
        let prefix_len = these_components
            .iter()
            .zip(other_components.iter())
            .take_while(|(a, b)| a == b)
            .count();
        #[cfg(windows)]
        debug_assert!(
            prefix_len >= 1,
            "Cannot traverse drives between {} and {}",
            self,
            other
        );

        let traverse_count = these_components.len() - prefix_len;
        // For every remaining non-matching segment in self, add a directory traversal
        // Then, add every non-matching segment from other
        let path = std::iter::repeat(Component::ParentDir)
            .take(traverse_count)
            .chain(other_components.into_iter().skip(prefix_len))
            .collect::<PathBuf>();
        AnchoredSystemPathBuf(path)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::*;

    #[test]
    fn test_relative_path_to() {
        #[cfg(unix)]
        let root_token = "/";
        #[cfg(windows)]
        let root_token = "C:\\";

        let root = AbsoluteSystemPathBuf::new(
            [root_token, "a", "b", "c"].join(std::path::MAIN_SEPARATOR_STR),
        )
        .unwrap();

        // /a/b/c
        // vs
        // /a -> ../..
        // /a/b/d -> ../d
        // /a/b/c/d -> d
        // /e/f -> ../../../e/f
        // / -> ../../..
        let test_cases: &[(&[&str], &[&str])] = &[
            (&["a"], &["..", ".."]),
            (&["a", "b", "d"], &["..", "d"]),
            (&["a", "b", "c", "d"], &["d"]),
            (&["e", "f"], &["..", "..", "..", "e", "f"]),
            (&[], &["..", "..", ".."]),
        ];
        for (input, expected) in test_cases {
            let mut parts = vec![root_token];
            parts.extend_from_slice(input);
            let target =
                AbsoluteSystemPathBuf::new(parts.join(std::path::MAIN_SEPARATOR_STR)).unwrap();
            let expected =
                AnchoredSystemPathBuf::from_raw(expected.join(std::path::MAIN_SEPARATOR_STR))
                    .unwrap();
            let result = root.relative_path_to(&target);
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn test_create_absolute_path() -> Result<()> {
        #[cfg(unix)]
        {
            let absolute_path = AbsoluteSystemPath::new("/foo/bar")?;
            assert_eq!(absolute_path.to_string(), "/foo/bar");
        }

        #[cfg(windows)]
        {
            let absolute_path = AbsoluteSystemPath::new(r"C:\foo\bar")?;
            assert_eq!(absolute_path.to_string(), r"C:\foo\bar");
        }

        Ok(())
    }
}
