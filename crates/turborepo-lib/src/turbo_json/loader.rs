use std::collections::HashMap;

use tracing::debug;
use turbopath::{AbsoluteSystemPath, AbsoluteSystemPathBuf};
use turborepo_errors::Spanned;
use turborepo_repository::{
    package_graph::{PackageInfo, PackageName},
    package_json::PackageJson,
};

use super::{Pipeline, RawTaskDefinition, TurboJson, CONFIG_FILE};
use crate::{
    config::Error,
    run::{task_access::TASK_ACCESS_CONFIG_PATH, task_id::TaskName},
};

/// Structure for loading TurboJson structures.
/// Depending on the strategy used, TurboJson might not correspond to
/// `turbo.json` file.
#[derive(Debug, Clone)]
pub struct TurboJsonLoader {
    repo_root: AbsoluteSystemPathBuf,
    strategy: Strategy,
}

#[derive(Debug, Clone)]
enum Strategy {
    SinglePackage {
        root_turbo_json: AbsoluteSystemPathBuf,
        package_json: PackageJson,
    },
    Workspace {
        // Map of package names to their package specific turbo.json
        packages: HashMap<PackageName, AbsoluteSystemPathBuf>,
    },
    TaskAccess {
        root_turbo_json: AbsoluteSystemPathBuf,
        package_json: PackageJson,
    },
}

impl TurboJsonLoader {
    /// Create a loader that will load turbo.json files throughout the workspace
    pub fn workspace(
        repo_root: AbsoluteSystemPathBuf,
        packages: HashMap<PackageName, AbsoluteSystemPathBuf>,
    ) -> Self {
        Self {
            repo_root,
            strategy: Strategy::Workspace { packages },
        }
    }

    /// Create a loader that will load a root turbo.json or synthesize one if
    /// the file doesn't exist
    pub fn single_package(
        repo_root: AbsoluteSystemPathBuf,
        root_turbo_json: AbsoluteSystemPathBuf,
        package_json: PackageJson,
    ) -> Self {
        Self {
            repo_root,
            strategy: Strategy::SinglePackage {
                root_turbo_json,
                package_json,
            },
        }
    }

    /// Create a loader that will load a root turbo.json or synthesize one if
    /// the file doesn't exist
    pub fn task_access(
        repo_root: AbsoluteSystemPathBuf,
        root_turbo_json: AbsoluteSystemPathBuf,
        package_json: PackageJson,
    ) -> Self {
        Self {
            repo_root,
            strategy: Strategy::TaskAccess {
                root_turbo_json,
                package_json,
            },
        }
    }

    /// Load a turbo.json for a given package
    pub fn load(&self, package: &PackageName) -> Result<TurboJson, Error> {
        match &self.strategy {
            Strategy::SinglePackage {
                package_json,
                root_turbo_json,
            } => {
                if !matches!(package, PackageName::Root) {
                    Err(Error::InvalidTurboJsonLoad(package.clone()))
                } else {
                    load_from_root_package_json(&self.repo_root, root_turbo_json, package_json)
                }
            }
            Strategy::Workspace { packages } => {
                let path = packages.get(package).ok_or_else(|| Error::NoTurboJSON)?;
                load_from_file(&self.repo_root, path)
            }
            Strategy::TaskAccess {
                package_json,
                root_turbo_json,
            } => {
                if !matches!(package, PackageName::Root) {
                    Err(Error::InvalidTurboJsonLoad(package.clone()))
                } else {
                    load_task_access_trace_turbo_json(
                        &self.repo_root,
                        root_turbo_json,
                        package_json,
                    )
                }
            }
        }
    }
}

/// Map all packages in the package graph to their turbo.json path
pub fn package_turbo_jsons<'a>(
    repo_root: &AbsoluteSystemPath,
    root_turbo_json_path: AbsoluteSystemPathBuf,
    packages: impl Iterator<Item = (&'a PackageName, &'a PackageInfo)>,
) -> HashMap<PackageName, AbsoluteSystemPathBuf> {
    let mut package_turbo_jsons = HashMap::new();
    package_turbo_jsons.insert(PackageName::Root, root_turbo_json_path);
    package_turbo_jsons.extend(packages.filter_map(|(pkg, info)| {
        if pkg == &PackageName::Root {
            None
        } else {
            Some((
                pkg.clone(),
                repo_root
                    .resolve(info.package_path())
                    .join_component(CONFIG_FILE),
            ))
        }
    }));
    package_turbo_jsons
}

fn load_from_file(
    repo_root: &AbsoluteSystemPath,
    turbo_json_path: &AbsoluteSystemPath,
) -> Result<TurboJson, Error> {
    match TurboJson::read(repo_root, turbo_json_path) {
        // If the file didn't exist, throw a custom error here instead of propagating
        Err(Error::Io(_)) => Err(Error::NoTurboJSON),
        // There was an error, and we don't have any chance of recovering
        // because we aren't synthesizing anything
        Err(e) => Err(e),
        // We're not synthesizing anything and there was no error, we're done
        Ok(turbo) => Ok(turbo),
    }
}

fn load_from_root_package_json(
    repo_root: &AbsoluteSystemPath,
    turbo_json_path: &AbsoluteSystemPath,
    root_package_json: &PackageJson,
) -> Result<TurboJson, Error> {
    let mut turbo_json = match TurboJson::read(repo_root, turbo_json_path) {
        // we're synthesizing, but we have a starting point
        // Note: this will have to change to support task inference in a monorepo
        // for now, we're going to error on any "root" tasks and turn non-root tasks into root
        // tasks
        Ok(mut turbo_json) => {
            let mut pipeline = Pipeline::default();
            for (task_name, task_definition) in turbo_json.tasks {
                if task_name.is_package_task() {
                    let (span, text) = task_definition.span_and_text("turbo.json");

                    return Err(Error::PackageTaskInSinglePackageMode {
                        task_id: task_name.to_string(),
                        span,
                        text,
                    });
                }

                pipeline.insert(task_name.into_root_task(), task_definition);
            }

            turbo_json.tasks = pipeline;

            turbo_json
        }
        // turbo.json doesn't exist, but we're going try to synthesize something
        Err(Error::Io(_)) => TurboJson::default(),
        // some other happened, we can't recover
        Err(e) => {
            return Err(e);
        }
    };

    // TODO: Add location info from package.json
    for script_name in root_package_json.scripts.keys() {
        let task_name = TaskName::from(script_name.as_str());
        if !turbo_json.has_task(&task_name) {
            let task_name = task_name.into_root_task();
            // Explicitly set cache to Some(false) in this definition
            // so we can pretend it was set on purpose. That way it
            // won't get clobbered by the merge function.
            turbo_json.tasks.insert(
                task_name,
                Spanned::new(RawTaskDefinition {
                    cache: Some(Spanned::new(false)),
                    ..RawTaskDefinition::default()
                }),
            );
        }
    }

    Ok(turbo_json)
}

fn load_task_access_trace_turbo_json(
    repo_root: &AbsoluteSystemPath,
    turbo_json_path: &AbsoluteSystemPath,
    root_package_json: &PackageJson,
) -> Result<TurboJson, Error> {
    let trace_json_path = repo_root.join_components(&TASK_ACCESS_CONFIG_PATH);
    let turbo_from_trace = TurboJson::read(repo_root, &trace_json_path);

    // check the zero config case (turbo trace file, but no turbo.json file)
    if let Ok(turbo_from_trace) = turbo_from_trace {
        if !turbo_json_path.exists() {
            debug!("Using turbo.json synthesized from trace file");
            return Ok(turbo_from_trace);
        }
    }
    load_from_root_package_json(repo_root, turbo_json_path, root_package_json)
}

#[cfg(test)]
mod test {
    use std::{collections::BTreeMap, fs};

    use anyhow::Result;
    use tempfile::tempdir;
    use test_case::test_case;

    use super::*;
    use crate::{task_graph::TaskDefinition, turbo_json::CONFIG_FILE};

    #[test_case(r"{}", TurboJson::default() ; "empty")]
    #[test_case(r#"{ "globalDependencies": ["tsconfig.json", "jest.config.js"] }"#,
        TurboJson {
            global_deps: vec!["jest.config.js".to_string(), "tsconfig.json".to_string()],
            ..TurboJson::default()
        }
    ; "global dependencies (sorted)")]
    #[test_case(r#"{ "globalPassThroughEnv": ["GITHUB_TOKEN", "AWS_SECRET_KEY"] }"#,
        TurboJson {
            global_pass_through_env: Some(vec!["AWS_SECRET_KEY".to_string(), "GITHUB_TOKEN".to_string()]),
            ..TurboJson::default()
        }
    )]
    #[test_case(r#"{ "//": "A comment"}"#, TurboJson::default() ; "faux comment")]
    #[test_case(r#"{ "//": "A comment", "//": "Another comment" }"#, TurboJson::default() ; "two faux comments")]
    fn test_get_root_turbo_no_synthesizing(
        turbo_json_content: &str,
        expected_turbo_json: TurboJson,
    ) -> Result<()> {
        let root_dir = tempdir()?;
        let repo_root = AbsoluteSystemPath::from_std_path(root_dir.path())?;
        let root_turbo_json = repo_root.join_component("turbo.json");
        fs::write(&root_turbo_json, turbo_json_content)?;
        let loader = TurboJsonLoader::workspace(
            repo_root.to_owned(),
            vec![(PackageName::Root, root_turbo_json)]
                .into_iter()
                .collect(),
        );

        let mut turbo_json = loader.load(&PackageName::Root)?;

        turbo_json.text = None;
        turbo_json.path = None;
        assert_eq!(turbo_json, expected_turbo_json);

        Ok(())
    }

    #[test_case(
        None,
        PackageJson {
             scripts: [("build".to_string(), Spanned::new("echo build".to_string()))].into_iter().collect(),
             ..PackageJson::default()
        },
        TurboJson {
            tasks: Pipeline([(
                "//#build".into(),
                Spanned::new(RawTaskDefinition {
                    cache: Some(Spanned::new(false)),
                    ..RawTaskDefinition::default()
                })
              )].into_iter().collect()
            ),
            ..TurboJson::default()
        }
    )]
    #[test_case(
        Some(r#"{
            "tasks": {
                "build": {
                    "cache": true
                }
            }
        }"#),
        PackageJson {
             scripts: [("test".to_string(), Spanned::new("echo test".to_string()))].into_iter().collect(),
             ..PackageJson::default()
        },
        TurboJson {
            tasks: Pipeline([(
                "//#build".into(),
                Spanned::new(RawTaskDefinition {
                    cache: Some(Spanned::new(true).with_range(81..85)),
                    ..RawTaskDefinition::default()
                }).with_range(50..103)
            ),
            (
                "//#test".into(),
                Spanned::new(RawTaskDefinition {
                     cache: Some(Spanned::new(false)),
                    ..RawTaskDefinition::default()
                })
            )].into_iter().collect()),
            ..TurboJson::default()
        }
    )]
    fn test_get_root_turbo_with_synthesizing(
        turbo_json_content: Option<&str>,
        root_package_json: PackageJson,
        expected_turbo_json: TurboJson,
    ) -> Result<()> {
        let root_dir = tempdir()?;
        let repo_root = AbsoluteSystemPath::from_std_path(root_dir.path())?;
        let root_turbo_json = repo_root.join_component(CONFIG_FILE);

        if let Some(content) = turbo_json_content {
            fs::write(&root_turbo_json, content)?;
        }

        let loader = TurboJsonLoader::single_package(
            repo_root.to_owned(),
            root_turbo_json,
            root_package_json,
        );
        let mut turbo_json = loader.load(&PackageName::Root)?;
        turbo_json.text = None;
        turbo_json.path = None;
        for (_, task_definition) in turbo_json.tasks.iter_mut() {
            task_definition.path = None;
            task_definition.text = None;
        }
        assert_eq!(turbo_json, expected_turbo_json);

        Ok(())
    }

    #[test_case(
        Some(r#"{ "tasks": {"//#build": {"env": ["SPECIAL_VAR"]}} }"#),
        Some(r#"{ "tasks": {"build": {"env": ["EXPLICIT_VAR"]}} }"#),
        TaskDefinition { env: vec!["EXPLICIT_VAR".to_string()], .. Default::default() }
    ; "both present")]
    #[test_case(
        None,
        Some(r#"{ "tasks": {"build": {"env": ["EXPLICIT_VAR"]}} }"#),
        TaskDefinition { env: vec!["EXPLICIT_VAR".to_string()], .. Default::default() }
    ; "no trace")]
    #[test_case(
        Some(r#"{ "tasks": {"//#build": {"env": ["SPECIAL_VAR"]}} }"#),
        None,
        TaskDefinition { env: vec!["SPECIAL_VAR".to_string()], .. Default::default() }
    ; "no turbo.json")]
    #[test_case(
        None,
        None,
        TaskDefinition { cache: false, .. Default::default() }
    ; "both missing")]
    fn test_task_access_loading(
        trace_contents: Option<&str>,
        turbo_json_content: Option<&str>,
        expected_root_build: TaskDefinition,
    ) -> Result<()> {
        let root_dir = tempdir()?;
        let repo_root = AbsoluteSystemPath::from_std_path(root_dir.path())?;
        let root_turbo_json = repo_root.join_component(CONFIG_FILE);

        if let Some(content) = turbo_json_content {
            root_turbo_json.create_with_contents(content.as_bytes())?;
        }
        if let Some(content) = trace_contents {
            let trace_path = repo_root.join_components(&TASK_ACCESS_CONFIG_PATH);
            trace_path.ensure_dir()?;
            trace_path.create_with_contents(content.as_bytes())?;
        }

        let mut scripts = BTreeMap::new();
        scripts.insert("build".into(), Spanned::new("echo building".into()));
        let root_package_json = PackageJson {
            scripts,
            ..Default::default()
        };

        let loader =
            TurboJsonLoader::task_access(repo_root.to_owned(), root_turbo_json, root_package_json);
        let turbo_json = loader.load(&PackageName::Root)?;
        let root_build = turbo_json
            .tasks
            .get(&TaskName::from("//#build"))
            .expect("root build should always exist")
            .as_inner();

        assert_eq!(
            expected_root_build,
            TaskDefinition::try_from(root_build.clone())?
        );

        Ok(())
    }

    #[test]
    fn test_single_package_loading_non_root() {
        let junk_path = AbsoluteSystemPath::new(if cfg!(windows) {
            "C:\\never\\loaded"
        } else {
            "/never/loaded"
        })
        .unwrap();
        let non_root = PackageName::from("some-pkg");
        let single_loader = TurboJsonLoader::single_package(
            junk_path.to_owned(),
            junk_path.to_owned(),
            PackageJson::default(),
        );
        let task_access_loader = TurboJsonLoader::task_access(
            junk_path.to_owned(),
            junk_path.to_owned(),
            PackageJson::default(),
        );

        for loader in [single_loader, task_access_loader] {
            let result = loader.load(&non_root);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                matches!(err, Error::InvalidTurboJsonLoad(_)),
                "expected {err} to be no turbo json"
            );
        }
    }

    #[test]
    fn test_workspace_turbo_json_loading() {
        let root_dir = tempdir().unwrap();
        let repo_root = AbsoluteSystemPath::from_std_path(root_dir.path()).unwrap();
        let a_turbo_json = repo_root.join_components(&["packages", "a", "turbo.json"]);
        a_turbo_json.ensure_dir().unwrap();
        let turbo_jsons = vec![(PackageName::from("a"), a_turbo_json.clone())]
            .into_iter()
            .collect();

        let loader = TurboJsonLoader::workspace(repo_root.to_owned(), turbo_jsons);
        let result = loader.load(&PackageName::from("a"));
        assert!(
            matches!(result.unwrap_err(), Error::NoTurboJSON),
            "expected parsing to fail with missing turbo.json"
        );

        a_turbo_json
            .create_with_contents(r#"{"tasks": {"build": {}}}"#)
            .unwrap();

        let turbo_json = loader.load(&PackageName::from("a")).unwrap();
        assert_eq!(turbo_json.tasks.len(), 1);
    }
}
