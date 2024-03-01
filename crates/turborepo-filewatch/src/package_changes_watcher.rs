use std::collections::HashSet;

use ignore::gitignore::Gitignore;
use notify::Event;
use tokio::sync::{broadcast, oneshot, watch};
use turbopath::{AbsoluteSystemPathBuf, AnchoredSystemPath};
use turborepo_repository::{
    change_mapper::{ChangeMapper, DefaultPackageChangeMapper, PackageChanges},
    package_graph::{PackageGraph, PackageGraphBuilder, PackageName},
    package_json::PackageJson,
};

use crate::{NotifyError, OptionalWatch};

pub enum PackageChangeEvent {
    // We might want to make this just String
    Package { name: PackageName },
    Rediscover,
}

/// Watches for changes to a package's files and directories.
pub struct PackageChangesWatcher {
    _exit_tx: oneshot::Sender<()>,
    _handle: tokio::task::JoinHandle<()>,
    package_change_events_rx: watch::Receiver<PackageChangeEvent>,
}

impl PackageChangesWatcher {
    pub fn new(
        repo_root: AbsoluteSystemPathBuf,
        file_events_lazy: OptionalWatch<broadcast::Receiver<Result<Event, NotifyError>>>,
    ) -> Self {
        let (exit_tx, exit_rx) = oneshot::channel();
        let (package_change_events_tx, package_change_events_rx) =
            watch::channel(PackageChangeEvent::Rediscover);
        let subscriber = Subscriber::new(repo_root, file_events_lazy, package_change_events_tx);

        let _handle = tokio::spawn(subscriber.watch(exit_rx));
        Self {
            _exit_tx: exit_tx,
            _handle,
            package_change_events_rx,
        }
    }

    pub fn package_changes(&self) -> watch::Receiver<PackageChangeEvent> {
        self.package_change_events_rx.clone()
    }
}

struct Subscriber {
    file_events_lazy: OptionalWatch<broadcast::Receiver<Result<Event, NotifyError>>>,
    repo_root: AbsoluteSystemPathBuf,
    package_change_events_tx: watch::Sender<PackageChangeEvent>,
}

// This is a workaround because `ignore` doesn't match against a path's
// ancestors, i.e. if we have `foo/bar/baz` and the .gitignore has `foo/`, it
// won't match.
fn ancestors_is_ignored(gitignore: &Gitignore, path: &AnchoredSystemPath) -> bool {
    path.ancestors().enumerate().any(|(idx, p)| {
        let is_dir = idx != 0;
        gitignore.matched(p, is_dir).is_ignore()
    })
}

struct RepoState {
    file_events: broadcast::Receiver<Result<Event, NotifyError>>,
    pkg_dep_graph: PackageGraph,
}

impl Subscriber {
    fn new(
        repo_root: AbsoluteSystemPathBuf,
        file_events_lazy: OptionalWatch<broadcast::Receiver<Result<Event, NotifyError>>>,
        package_change_events_tx: watch::Sender<PackageChangeEvent>,
    ) -> Self {
        Subscriber {
            repo_root,
            file_events_lazy,
            package_change_events_tx,
        }
    }

    async fn initialize_repo_state(&mut self) -> Option<RepoState> {
        let Ok(file_events) = self.file_events_lazy.get().await.map(|r| r.resubscribe()) else {
            // if we get here, it means that file watching has not started, so we should
            // just report that the package watcher is not available
            tracing::debug!("file watching shut down, package watcher not available");
            return None;
        };

        let Ok(root_package_json) =
            PackageJson::load(&self.repo_root.join_component("package.json"))
        else {
            tracing::debug!("no package.json found, package watcher not available");
            return None;
        };

        let Ok(pkg_dep_graph) = PackageGraphBuilder::new(&self.repo_root, root_package_json)
            .build()
            .await
        else {
            tracing::debug!("package graph not available, package watcher not available");
            return None;
        };

        Some(RepoState {
            file_events,
            pkg_dep_graph,
        })
    }

    async fn watch(mut self, exit_rx: oneshot::Receiver<()>) {
        let process = async {
            let Some(mut repo_state) = self.initialize_repo_state().await else {
                return;
            };
            // TODO: Pass in global_deps and ignore_patterns
            let mut change_mapper = ChangeMapper::new(
                &repo_state.pkg_dep_graph,
                vec![],
                DefaultPackageChangeMapper::new(&repo_state.pkg_dep_graph),
            );

            loop {
                match repo_state.file_events.recv().await {
                    Ok(Ok(Event { paths, .. })) => {
                        // No point in raising an error for an invalid .gitignore
                        // This is slightly incorrect because we should also search for the
                        // .gitignore files in the workspaces.
                        let (root_gitignore, _) =
                            Gitignore::new(&self.repo_root.join_component(".gitignore"));

                        let changed_files: HashSet<_> = paths
                            .into_iter()
                            .filter_map(|p| {
                                let p = AbsoluteSystemPathBuf::try_from(p).ok()?;
                                self.repo_root.anchor(p).ok()
                            })
                            .filter(|p| !ancestors_is_ignored(&root_gitignore, p))
                            .collect();

                        let changes = change_mapper.changed_packages(changed_files.clone(), None);

                        match changes {
                            Ok(PackageChanges::All) => {
                                // We tell the client that we need to rediscover the packages, i.e.
                                // all bets are off, just re-run everything
                                let _ = self
                                    .package_change_events_tx
                                    .send(PackageChangeEvent::Rediscover);
                                match self.initialize_repo_state().await {
                                    Some(new_repo_state) => {
                                        repo_state = new_repo_state;
                                        change_mapper = ChangeMapper::new(
                                            &repo_state.pkg_dep_graph,
                                            vec![],
                                            DefaultPackageChangeMapper::new(
                                                &repo_state.pkg_dep_graph,
                                            ),
                                        );
                                    }
                                    None => {
                                        break;
                                    }
                                }
                            }
                            Ok(PackageChanges::Some(changed_pkgs)) => {
                                tracing::debug!(
                                    "changed files: {:?} changed packages: {:?}",
                                    changed_files,
                                    changed_pkgs
                                );
                                for pkg in changed_pkgs {
                                    let _ = self.package_change_events_tx.send(
                                        PackageChangeEvent::Package {
                                            name: pkg.name.clone(),
                                        },
                                    );
                                }
                            }
                            Err(err) => {
                                tracing::error!("error: {:?}", err);
                            }
                        }
                    }
                    Ok(Err(err)) => {
                        tracing::error!("file event error: {:?}", err);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        tracing::warn!("file event lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("file event channel closed");
                        break;
                    }
                }
            }
        };

        tokio::select! {
            biased;
            _ = exit_rx => {
                tracing::debug!("exiting package changes watcher due to signal");
            },
            _ = process => {
                tracing::debug!("exiting package changes watcher due to process end");
            }
        }
    }
}
