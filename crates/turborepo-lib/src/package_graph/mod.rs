use std::{fmt, rc::Rc};

use anyhow::Result;
use turbopath::{AbsoluteSystemPath, AbsoluteSystemPathBuf};
use turborepo_lockfiles::Lockfile;

use crate::{package_json::PackageJson, package_manager::PackageManager};

#[derive(Default)]
pub struct WorkspaceCatalog {}

pub struct PackageGraph {
    workspace_graph: Rc<petgraph::Graph<String, String>>,
    #[allow(dead_code)]
    workspace_infos: Rc<WorkspaceCatalog>,
    package_manager: PackageManager,
    lockfile: Box<dyn Lockfile>,
}

impl PackageGraph {
    pub fn build_single_package_graph(_root_package_json: &PackageJson) -> Result<PackageGraph> {
        // TODO
        Ok(PackageGraph {
            workspace_graph: Rc::new(petgraph::Graph::new()),
            workspace_infos: Rc::new(WorkspaceCatalog::default()),
            package_manager: PackageManager::Npm,
            lockfile: Box::<turborepo_lockfiles::NpmLockfile>::default(),
        })
    }

    pub fn build_multi_package_graph(
        _repo_root: &AbsoluteSystemPathBuf,
        _root_package_json: &PackageJson,
    ) -> Result<PackageGraph> {
        // TODO
        Ok(PackageGraph {
            workspace_graph: Rc::new(petgraph::Graph::new()),
            workspace_infos: Rc::new(WorkspaceCatalog::default()),
            package_manager: PackageManager::Npm,
            lockfile: Box::<turborepo_lockfiles::NpmLockfile>::default(),
        })
    }

    pub fn validate(&self) -> Result<()> {
        // TODO
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.workspace_graph.node_count()
    }

    pub fn package_manager(&self) -> &PackageManager {
        &self.package_manager
    }

    pub fn lockfile(&self) -> &dyn Lockfile {
        self.lockfile.as_ref()
    }
}

struct DependencyVersion<'a> {
    protocol: Option<&'a str>,
    version: &'a str,
}

impl<'a> DependencyVersion<'a> {
    fn new(qualified_version: &'a str) -> Self {
        qualified_version.split_once(':').map_or(
            Self {
                protocol: None,
                version: qualified_version,
            },
            |(protocol, version)| Self {
                protocol: Some(protocol),
                version,
            },
        )
    }

    fn is_external(&self) -> bool {
        // The npm protocol for yarn by default still uses the workspace package if the
        // workspace version is in a compatible semver range. See https://github.com/yarnpkg/berry/discussions/4015
        // For now, we will just assume if the npm protocol is being used and the
        // version matches its an internal dependency which matches the existing
        // behavior before this additional logic was added.

        // TODO: extend this to support the `enableTransparentWorkspaces` yarn option
        self.protocol.map_or(false, |p| p != "npm")
    }

    fn matches_workspace_package(
        &self,
        package_version: &str,
        cwd: &AbsoluteSystemPath,
        root: &AbsoluteSystemPath,
    ) -> bool {
        match self.protocol {
            Some("workspace") => true,
            Some("file") | Some("link") => {
                let dependency_path = cwd.join_component(&self.to_string());
                root.contains(&dependency_path)
            }
            Some(_) if self.is_external() => {
                // Other protocols are assumed to be external references ("github:", etc)
                false
            }
            _ if self.version == "*" => true,
            _ => {
                // If we got this far, then we need to check the workspace package version to
                // see it satisfies the dependencies range to determin whether
                // or not its an internal or external dependency.
                let constraint = node_semver::Range::parse(self.version);
                let version = node_semver::Version::parse(package_version);

                // For backwards compatibility with existing behavior, if we can't parse the
                // version then we treat the dependency as an internal package
                // reference and swallow the error.

                // TODO: some package managers also support tags like "latest". Does extra
                // handling need to be added for this corner-case
                constraint
                    .ok()
                    .zip(version.ok())
                    .map_or(true, |(constraint, version)| constraint.satisfies(&version))
            }
        }
    }
}

impl<'a> fmt::Display for DependencyVersion<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.protocol {
            Some(protocol) => f.write_fmt(format_args!("{}:{}", protocol, self.version)),
            None => f.write_str(self.version),
        }
    }
}

#[cfg(test)]
mod test {
    use test_case::test_case;

    use super::*;

    #[test_case("1.2.3", "1.2.3", true ; "handles exact match")]
    #[test_case("1.2.3", "^1.0.0", true ; "handles semver range satisfied")]
    #[test_case("2.3.4", "^1.0.0", false ; "handles semver range not satisfied")]
    #[test_case("1.2.3", "workspace:1.2.3", true ; "handles workspace protocol with version")]
    #[test_case("1.2.3", "workspace:../other-packages/", true ; "handles workspace protocol with relative path")]
    #[test_case("1.2.3", "npm:^1.2.3", true ; "handles npm protocol with satisfied semver range")]
    #[test_case("2.3.4", "npm:^1.2.3", false ; "handles npm protocol with not satisfied semver range")]
    #[test_case("1.2.3", "1.2.2-alpha-123abcd.0", false ; "handles pre-release versions")]
    // for backwards compatability with the code before versions were verified
    #[test_case("sometag", "1.2.3", true ; "handles non-semver package version")]
    // for backwards compatability with the code before versions were verified
    #[test_case("1.2.3", "sometag", true ; "handles non-semver dependency version")]
    #[test_case("1.2.3", "file:../libB", true ; "handles file:.. inside repo")]
    #[test_case("1.2.3", "file:../../../otherproject", false ; "handles file:.. outside repo")]
    #[test_case("1.2.3", "link:../libB", true ; "handles link:.. inside repo")]
    #[test_case("1.2.3", "link:../../../otherproject", false ; "handles link:.. outside repo")]
    #[test_case("0.0.0-development", "*", true ; "handles development versions")]
    fn test_matches_workspace_package(package_version: &str, range: &str, expected: bool) {
        let root = AbsoluteSystemPathBuf::new(if cfg!(windows) {
            "C:\\some\\repo"
        } else {
            "/some/repo"
        })
        .unwrap();
        let pkg_dir = root.join_components(&["packages", "libA"]);

        assert_eq!(
            DependencyVersion::new(range).matches_workspace_package(
                package_version,
                &pkg_dir,
                &root
            ),
            expected
        );
    }
}
