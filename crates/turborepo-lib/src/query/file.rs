use std::sync::Arc;

use async_graphql::{Object, SimpleObject, Union};
use itertools::Itertools;
use turbo_trace::Tracer;
use turbopath::AbsoluteSystemPathBuf;
use turborepo_repository::{
    change_mapper::{ChangeMapper, GlobalDepsPackageChangeMapper},
    package_graph::PackageNode,
};

use crate::{
    query::{package::Package, Array, Error, PackageChangeReason},
    run::Run,
};

pub struct File {
    run: Arc<Run>,
    path: AbsoluteSystemPathBuf,
}

impl File {
    pub fn new(run: Arc<Run>, path: AbsoluteSystemPathBuf) -> Self {
        Self { run, path }
    }
}

#[derive(SimpleObject, Debug)]
pub struct TraceError {
    message: String,
    path: Option<String>,
    start: Option<usize>,
    end: Option<usize>,
}

impl From<turbo_trace::TraceError> for TraceError {
    fn from(error: turbo_trace::TraceError) -> Self {
        let message = error.to_string();
        match error {
            turbo_trace::TraceError::FileNotFound(file) => TraceError {
                message,
                path: Some(file.to_string()),
                start: None,
                end: None,
            },
            turbo_trace::TraceError::PathEncoding(_) => TraceError {
                message,
                path: None,
                start: None,
                end: None,
            },
            turbo_trace::TraceError::RootFile(path) => TraceError {
                message,
                path: Some(path.to_string()),
                start: None,
                end: None,
            },
            turbo_trace::TraceError::Resolve { span, text } => TraceError {
                message,
                path: Some(text.name().to_string()),
                start: Some(span.offset()),
                end: Some(span.offset() + span.len()),
            },
        }
    }
}

#[derive(SimpleObject)]
struct TraceResult {
    files: Array<File>,
    errors: Array<TraceError>,
}

impl TraceResult {
    fn new(result: turbo_trace::TraceResult, run: Arc<Run>) -> Self {
        Self {
            files: result
                .files
                .into_iter()
                .sorted()
                .map(|path| File::new(run.clone(), path))
                .collect(),
            errors: result.errors.into_iter().map(|e| e.into()).collect(),
        }
    }
}

#[derive(SimpleObject)]
struct All {
    reason: PackageChangeReason,
    count: usize,
}

#[derive(Union)]
enum PackageMapping {
    All(All),
    Package(Package),
}

impl File {
    fn get_package(&self) -> Result<Option<PackageMapping>, Error> {
        let change_mapper = ChangeMapper::new(
            self.run.pkg_dep_graph(),
            vec![],
            GlobalDepsPackageChangeMapper::new(
                self.run.pkg_dep_graph(),
                self.run
                    .root_turbo_json()
                    .global_deps
                    .iter()
                    .map(|dep| dep.as_str()),
            )?,
        );

        // If the file is not in the repo, we can't get the package
        let Ok(anchored_path) = self.run.repo_root().anchor(&self.path) else {
            return Ok(None);
        };

        let package = change_mapper
            .package_detector()
            .detect_package(&anchored_path);

        match package {
            turborepo_repository::change_mapper::PackageMapping::All(reason) => {
                Ok(Some(PackageMapping::All(All {
                    reason: reason.into(),
                    count: self.run.pkg_dep_graph().len(),
                })))
            }
            turborepo_repository::change_mapper::PackageMapping::Package((package, _)) => {
                Ok(Some(PackageMapping::Package(Package {
                    run: self.run.clone(),
                    name: package.name.clone(),
                })))
            }
            turborepo_repository::change_mapper::PackageMapping::None => Ok(None),
        }
    }
}

#[Object]
impl File {
    async fn contents(&self) -> Result<String, Error> {
        Ok(self.path.read_to_string()?)
    }

    // This is `Option` because the file may not be in the repo
    async fn path(&self) -> Option<String> {
        self.run
            .repo_root()
            .anchor(&self.path)
            .ok()
            .map(|path| path.to_string())
    }

    async fn absolute_path(&self) -> String {
        self.path.to_string()
    }

    async fn package(&self) -> Result<Option<PackageMapping>, Error> {
        self.get_package()
    }

    /// Gets the affected packages for the file, i.e. all packages that depend
    /// on the file.
    async fn affected_packages(&self) -> Result<Array<Package>, Error> {
        match self.get_package() {
            Ok(Some(PackageMapping::All(_))) => Ok(self
                .run
                .pkg_dep_graph()
                .packages()
                .map(|(name, _)| Package {
                    run: self.run.clone(),
                    name: name.clone(),
                })
                .sorted_by(|a, b| a.name.cmp(&b.name))
                .collect()),
            Ok(Some(PackageMapping::Package(package))) => {
                let node: PackageNode = PackageNode::Workspace(package.name.clone());
                Ok(self
                    .run
                    .pkg_dep_graph()
                    .ancestors(&node)
                    .iter()
                    .map(|package| Package {
                        run: self.run.clone(),
                        name: package.as_package_name().clone(),
                    })
                    // Add the package itself to the list
                    .chain(std::iter::once(Package {
                        run: self.run.clone(),
                        name: package.name.clone(),
                    }))
                    .sorted_by(|a, b| a.name.cmp(&b.name))
                    .collect())
            }
            Ok(None) => Ok(Array::new()),
            Err(e) => Err(e),
        }
    }

    async fn dependencies(&self) -> TraceResult {
        let tracer = Tracer::new(
            self.run.repo_root().to_owned(),
            vec![self.path.clone()],
            None,
        );

        let mut result = tracer.trace();
        // Remove the file itself from the result
        result.files.remove(&self.path);
        TraceResult::new(result, self.run.clone())
    }
}
