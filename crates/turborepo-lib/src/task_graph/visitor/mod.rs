mod error;
mod exec;

use std::{
    borrow::Cow,
    collections::HashSet,
    io::Write,
    sync::{Arc, Mutex, OnceLock},
};

use console::{Style, StyledObject};
use either::Either;
use error::{TaskError, TaskWarning};
use exec::ExecContextFactory;
use futures::{stream::FuturesUnordered, StreamExt};
use itertools::Itertools;
use miette::{Diagnostic, NamedSource, SourceSpan};
use regex::Regex;
use tokio::sync::mpsc;
use tracing::{debug, error, warn, Span};
use turbopath::{AbsoluteSystemPath, AnchoredSystemPath};
use turborepo_ci::{Vendor, VendorBehavior};
use turborepo_env::{platform::PlatformEnv, EnvironmentVariableMap};
use turborepo_repository::package_graph::{PackageGraph, PackageName, ROOT_PKG_NAME};
use turborepo_telemetry::events::{
    generic::GenericEventBuilder, task::PackageTaskEventBuilder, EventBuilder, TrackedErrors,
};
use turborepo_ui::{
    sender::{TaskSender, UISender},
    tui::event::CacheResult,
    ColorConfig, ColorSelector, OutputClient, OutputSink, OutputWriter, PrefixedUI,
};

use crate::{
    cli::EnvMode,
    engine::{Engine, ExecutionOptions},
    opts::RunOpts,
    process::ProcessManager,
    run::{
        global_hash::GlobalHashableInputs,
        summary::{self, GlobalHashSummary, RunTracker},
        task_access::TaskAccess,
        task_id::TaskId,
        CacheOutput, RunCache,
    },
    task_hash::{self, PackageInputsHashes, TaskHashTrackerState, TaskHasher},
};

// This holds the whole world
pub struct Visitor<'a> {
    color_cache: ColorSelector,
    dry: bool,
    global_env: EnvironmentVariableMap,
    global_env_mode: EnvMode,
    manager: ProcessManager,
    run_opts: &'a RunOpts,
    package_graph: Arc<PackageGraph>,
    repo_root: &'a AbsoluteSystemPath,
    run_cache: Arc<RunCache>,
    run_tracker: RunTracker,
    task_access: &'a TaskAccess,
    sink: OutputSink<StdWriter>,
    task_hasher: TaskHasher<'a>,
    color_config: ColorConfig,
    is_watch: bool,
    ui_sender: Option<UISender>,
    warnings: Arc<Mutex<Vec<TaskWarning>>>,
}

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum Error {
    #[error("cannot find package {package_name} for task {task_id}")]
    MissingPackage {
        package_name: PackageName,
        task_id: TaskId<'static>,
    },
    #[error(
        "root task {task_name} ({command}) looks like it invokes turbo and might cause a loop"
    )]
    RecursiveTurbo {
        task_name: String,
        command: String,
        #[label("task found here")]
        span: Option<SourceSpan>,
        #[source_code]
        text: NamedSource,
    },
    #[error("Could not find definition for task")]
    MissingDefinition,
    #[error("error while executing engine: {0}")]
    Engine(#[from] crate::engine::ExecuteError),
    #[error(transparent)]
    TaskHash(#[from] task_hash::Error),
    #[error(transparent)]
    RunSummary(#[from] summary::Error),
    #[error("internal errors encountered: {0}")]
    InternalErrors(String),
}

impl<'a> Visitor<'a> {
    // Disabling this lint until we stop adding state to the visitor.
    // Once we have the full picture we will go about grouping these pieces of data
    // together
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        package_graph: Arc<PackageGraph>,
        run_cache: Arc<RunCache>,
        run_tracker: RunTracker,
        task_access: &'a TaskAccess,
        run_opts: &'a RunOpts,
        package_inputs_hashes: PackageInputsHashes,
        env_at_execution_start: &'a EnvironmentVariableMap,
        global_hash: &'a str,
        global_env_mode: EnvMode,
        color_config: ColorConfig,
        manager: ProcessManager,
        repo_root: &'a AbsoluteSystemPath,
        global_env: EnvironmentVariableMap,
        ui_sender: Option<UISender>,
        is_watch: bool,
    ) -> Self {
        let task_hasher = TaskHasher::new(
            package_inputs_hashes,
            run_opts,
            env_at_execution_start,
            global_hash,
        );

        let sink = Self::sink(run_opts);
        let color_cache = ColorSelector::default();
        // Set up correct size for underlying pty

        if let Some(app) = ui_sender.as_ref() {
            if let Some(pane_size) = app.pane_size().await {
                manager.set_pty_size(pane_size.rows, pane_size.cols);
            }
        }

        Self {
            color_cache,
            dry: false,
            global_env_mode,
            manager,
            run_opts,
            package_graph,
            repo_root,
            run_cache,
            run_tracker,
            task_access,
            sink,
            task_hasher,
            color_config,
            global_env,
            ui_sender,
            is_watch,
            warnings: Default::default(),
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn visit(
        &self,
        engine: Arc<Engine>,
        telemetry: &GenericEventBuilder,
    ) -> Result<Vec<TaskError>, Error> {
        for task in engine.tasks().sorted() {
            self.color_cache.color_for_key(&task.to_string());
        }

        let concurrency = self.run_opts.concurrency as usize;
        let (node_sender, mut node_stream) = mpsc::channel(concurrency);

        let engine_handle = {
            let engine = engine.clone();
            tokio::spawn(engine.execute(ExecutionOptions::new(false, concurrency), node_sender))
        };
        let mut tasks = FuturesUnordered::new();
        let errors = Arc::new(Mutex::new(Vec::new()));
        let span = Span::current();

        let factory = ExecContextFactory::new(self, errors.clone(), self.manager.clone(), &engine);

        while let Some(message) = node_stream.recv().await {
            let span = tracing::debug_span!(parent: &span, "queue_task", task = %message.info);
            let _enter = span.enter();
            let crate::engine::Message { info, callback } = message;
            let package_name = PackageName::from(info.package());

            let workspace_info =
                self.package_graph
                    .package_info(&package_name)
                    .ok_or_else(|| Error::MissingPackage {
                        package_name: package_name.clone(),
                        task_id: info.clone(),
                    })?;

            let package_task_event =
                PackageTaskEventBuilder::new(info.package(), info.task()).with_parent(telemetry);
            let command = workspace_info
                .package_json
                .scripts
                .get(info.task())
                .cloned();

            match command {
                Some(cmd) if info.package() == ROOT_PKG_NAME && turbo_regex().is_match(&cmd) => {
                    package_task_event.track_error(TrackedErrors::RecursiveError);
                    let (span, text) = cmd.span_and_text("package.json");
                    return Err(Error::RecursiveTurbo {
                        task_name: info.to_string(),
                        command: cmd.to_string(),
                        span,
                        text,
                    });
                }
                _ => (),
            }

            let task_definition = engine
                .task_definition(&info)
                .ok_or(Error::MissingDefinition)?;

            let task_env_mode = task_definition.env_mode.unwrap_or(self.global_env_mode);
            package_task_event.track_env_mode(&task_env_mode.to_string());

            let dependency_set = engine.dependencies(&info).ok_or(Error::MissingDefinition)?;

            let task_hash_telemetry = package_task_event.child();
            let task_hash = self.task_hasher.calculate_task_hash(
                &info,
                task_definition,
                task_env_mode,
                workspace_info,
                dependency_set,
                task_hash_telemetry,
            )?;

            debug!("task {} hash is {}", info, task_hash);
            // We do this calculation earlier than we do in Go due to the `task_hasher`
            // being !Send. In the future we can look at doing this right before
            // task execution instead.
            let execution_env =
                self.task_hasher
                    .env(&info, task_env_mode, task_definition, &self.global_env)?;

            let task_cache = self.run_cache.task_cache(
                task_definition,
                workspace_info,
                info.clone(),
                &task_hash,
            );

            // Drop to avoid holding the span across an await
            drop(_enter);

            // here is where we do the logic split
            match self.dry {
                true => {
                    let dry_run_exec_context =
                        factory.dry_run_exec_context(info.clone(), task_cache);
                    let tracker = self.run_tracker.track_task(info.into_owned());
                    tasks.push(tokio::spawn(async move {
                        dry_run_exec_context.execute_dry_run(tracker).await
                    }));
                }
                false => {
                    // TODO(gsoltis): if/when we fix https://github.com/vercel/turborepo/issues/937
                    // the following block should never get hit. In the meantime, keep it after
                    // hashing so that downstream tasks can count on the hash existing
                    //
                    // bail if the script doesn't exist or is empty
                    if command.map_or(true, |s| s.is_empty()) {
                        continue;
                    }

                    let workspace_directory = self.repo_root.resolve(workspace_info.package_path());

                    let takes_input = task_definition.interactive || task_definition.persistent;
                    let mut exec_context = factory.exec_context(
                        info.clone(),
                        task_hash,
                        task_cache,
                        workspace_directory,
                        execution_env,
                        takes_input,
                        self.task_access.clone(),
                    );

                    let vendor_behavior =
                        Vendor::infer().and_then(|vendor| vendor.behavior.as_ref());

                    let output_client = if let Some(handle) = &self.ui_sender {
                        TaskOutput::UI(handle.task(info.to_string()))
                    } else {
                        TaskOutput::Direct(self.output_client(&info, vendor_behavior))
                    };

                    let tracker = self.run_tracker.track_task(info.clone().into_owned());
                    let spaces_client = self.run_tracker.spaces_task_client();
                    let parent_span = Span::current();
                    let execution_telemetry = package_task_event.child();

                    tasks.push(tokio::spawn(async move {
                        exec_context
                            .execute(
                                parent_span.id(),
                                tracker,
                                output_client,
                                callback,
                                spaces_client,
                                &execution_telemetry,
                            )
                            .await
                    }));
                }
            }
        }

        // Wait for the engine task to finish and for all of our tasks to finish
        engine_handle.await.expect("engine execution panicked")?;
        // This will poll the futures until they are all completed
        let mut internal_errors = Vec::new();
        while let Some(result) = tasks.next().await {
            if let Err(e) = result.unwrap_or_else(|e| panic!("task executor panicked: {e}")) {
                internal_errors.push(e);
            }
        }
        drop(factory);

        if !self.is_watch {
            if let Some(handle) = &self.ui_sender {
                handle.stop().await;
            }
        }

        if !internal_errors.is_empty() {
            return Err(Error::InternalErrors(
                internal_errors.into_iter().map(|e| e.to_string()).join(","),
            ));
        }

        // Write out the traced-config.json file if we have one
        self.task_access.save().await;

        let errors = Arc::into_inner(errors)
            .expect("only one strong reference to errors should remain")
            .into_inner()
            .expect("mutex poisoned");

        Ok(errors)
    }

    /// Finishes visiting the tasks, creates the run summary, and either
    /// prints, saves, or sends it to spaces.

    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(skip(
        self,
        packages,
        global_hash_inputs,
        engine,
        env_at_execution_start
    ))]
    pub(crate) async fn finish(
        self,
        exit_code: i32,
        packages: &HashSet<PackageName>,
        global_hash_inputs: GlobalHashableInputs<'_>,
        engine: &Engine,
        env_at_execution_start: &EnvironmentVariableMap,
        pkg_inference_root: Option<&AnchoredSystemPath>,
    ) -> Result<(), Error> {
        let Self {
            package_graph,
            color_config: ui,
            run_opts,
            repo_root,
            global_env_mode,
            task_hasher,
            is_watch,
            ..
        } = self;

        let global_hash_summary = GlobalHashSummary::try_from(global_hash_inputs)?;

        // output any warnings that we collected while running tasks
        if let Ok(warnings) = self.warnings.lock() {
            if !warnings.is_empty() {
                eprintln!();
                warn!("finished with warnings");
                eprintln!();

                PlatformEnv::output_header(global_env_mode == EnvMode::Strict, self.color_config);

                for warning in warnings.iter() {
                    PlatformEnv::output_for_task(
                        warning.missing_platform_env().to_owned(),
                        warning.task_id(),
                        self.color_config,
                    )
                }
            }
        }

        Ok(self
            .run_tracker
            .finish(
                exit_code,
                &package_graph,
                ui,
                repo_root,
                pkg_inference_root,
                run_opts,
                packages,
                global_hash_summary,
                global_env_mode,
                engine,
                task_hasher.task_hash_tracker(),
                env_at_execution_start,
                is_watch,
            )
            .await?)
    }

    fn sink(run_opts: &RunOpts) -> OutputSink<StdWriter> {
        let (out, err) = if run_opts.should_redirect_stderr_to_stdout() {
            (std::io::stdout().into(), std::io::stdout().into())
        } else {
            (std::io::stdout().into(), std::io::stderr().into())
        };
        OutputSink::new(out, err)
    }

    fn output_client(
        &self,
        task_id: &TaskId,
        vendor_behavior: Option<&VendorBehavior>,
    ) -> OutputClient<impl std::io::Write> {
        let behavior = match self.run_opts.log_order {
            crate::opts::ResolvedLogOrder::Stream if self.run_tracker.spaces_enabled() => {
                turborepo_ui::OutputClientBehavior::InMemoryBuffer
            }
            crate::opts::ResolvedLogOrder::Stream => {
                turborepo_ui::OutputClientBehavior::Passthrough
            }
            crate::opts::ResolvedLogOrder::Grouped => turborepo_ui::OutputClientBehavior::Grouped,
        };

        let mut logger = self.sink.logger(behavior);
        if let Some(vendor_behavior) = vendor_behavior {
            let group_name = if self.run_opts.single_package {
                task_id.task().to_string()
            } else {
                format!("{}:{}", task_id.package(), task_id.task())
            };

            let header_factory = (vendor_behavior.group_prefix)(group_name.to_owned());
            let footer_factory = (vendor_behavior.group_suffix)(group_name.to_owned());

            logger.with_header_footer(Some(header_factory), Some(footer_factory));

            let (error_header, error_footer) = (
                vendor_behavior
                    .error_group_prefix
                    .map(|f| f(group_name.to_owned())),
                vendor_behavior
                    .error_group_suffix
                    .map(|f| f(group_name.to_owned())),
            );
            logger.with_error_header_footer(error_header, error_footer);
        }
        logger
    }

    fn prefix<'b>(&self, task_id: &'b TaskId) -> Cow<'b, str> {
        match self.run_opts.log_prefix {
            crate::opts::ResolvedLogPrefix::Task if self.run_opts.single_package => {
                task_id.task().into()
            }
            crate::opts::ResolvedLogPrefix::Task => {
                format!("{}:{}", task_id.package(), task_id.task()).into()
            }
            crate::opts::ResolvedLogPrefix::None => "".into(),
        }
    }

    // Task ID as displayed in error messages
    fn display_task_id(&self, task_id: &TaskId) -> String {
        match self.run_opts.single_package {
            true => task_id.task().to_string(),
            false => task_id.to_string(),
        }
    }

    fn prefixed_ui<W: Write>(
        color_config: ColorConfig,
        is_github_actions: bool,
        stdout: W,
        stderr: W,
        prefix: StyledObject<String>,
    ) -> PrefixedUI<W> {
        let mut prefixed_ui = PrefixedUI::new(color_config, stdout, stderr)
            .with_output_prefix(prefix.clone())
            // TODO: we can probably come up with a more ergonomic way to achieve this
            .with_error_prefix(
                Style::new().apply_to(format!("{}ERROR: ", color_config.apply(prefix.clone()))),
            )
            .with_warn_prefix(prefix);
        if is_github_actions {
            prefixed_ui = prefixed_ui
                .with_error_prefix(Style::new().apply_to("[ERROR] ".to_string()))
                .with_warn_prefix(Style::new().apply_to("[WARN] ".to_string()));
        }
        prefixed_ui
    }

    /// Only used for the hashing comparison between Rust and Go. After port,
    /// should delete
    pub fn into_task_hash_tracker(self) -> TaskHashTrackerState {
        self.task_hasher.into_task_hash_tracker_state()
    }

    pub fn dry_run(&mut self) {
        self.dry = true;
        // No need to start a UI on dry run
        self.ui_sender = None;
    }
}

// A tiny enum that allows us to use the same type for stdout and stderr without
// the use of Box<dyn Write>
enum StdWriter {
    Out(std::io::Stdout),
    Err(std::io::Stderr),
    Null(std::io::Sink),
}

impl StdWriter {
    fn writer(&mut self) -> &mut dyn std::io::Write {
        match self {
            StdWriter::Out(out) => out,
            StdWriter::Err(err) => err,
            StdWriter::Null(null) => null,
        }
    }
}

impl From<std::io::Stdout> for StdWriter {
    fn from(value: std::io::Stdout) -> Self {
        Self::Out(value)
    }
}

impl From<std::io::Stderr> for StdWriter {
    fn from(value: std::io::Stderr) -> Self {
        Self::Err(value)
    }
}

impl From<std::io::Sink> for StdWriter {
    fn from(value: std::io::Sink) -> Self {
        Self::Null(value)
    }
}

impl std::io::Write for StdWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer().flush()
    }
}

/// Small wrapper over our two output types that defines a shared interface for
/// interacting with them.
enum TaskOutput<W> {
    Direct(OutputClient<W>),
    UI(TaskSender),
}

fn turbo_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s)turbo(?:$|\s)").unwrap())
}

/// Struct for displaying information about task's cache
enum TaskCacheOutput<W> {
    Direct(PrefixedUI<W>),
    UI(TaskSender),
}

impl<W: Write> TaskCacheOutput<W> {
    fn task_writer(&mut self) -> Either<turborepo_ui::PrefixedWriter<&mut W>, TaskSender> {
        match self {
            TaskCacheOutput::Direct(prefixed) => Either::Left(prefixed.output_prefixed_writer()),
            TaskCacheOutput::UI(task) => Either::Right(task.clone()),
        }
    }

    fn warn(&mut self, message: impl std::fmt::Display) {
        match self {
            TaskCacheOutput::Direct(prefixed) => prefixed.warn(message),
            TaskCacheOutput::UI(task) => {
                let _ = write!(task, "\r\n{message}\r\n");
            }
        }
    }
}

impl<W: Write> CacheOutput for TaskCacheOutput<W> {
    fn status(&mut self, message: &str, result: CacheResult) {
        match self {
            TaskCacheOutput::Direct(direct) => direct.output(message),
            TaskCacheOutput::UI(task) => task.status(message, result),
        }
    }

    fn error(&mut self, message: &str) {
        match self {
            TaskCacheOutput::Direct(prefixed) => prefixed.error(message),
            TaskCacheOutput::UI(task) => {
                let _ = write!(task, "{message}\r\n");
            }
        }
    }

    fn replay_logs(&mut self, log_file: &AbsoluteSystemPath) -> Result<(), turborepo_ui::Error> {
        match self {
            TaskCacheOutput::Direct(direct) => {
                let writer = direct.output_prefixed_writer();
                turborepo_ui::replay_logs(writer, log_file)
            }
            TaskCacheOutput::UI(task) => turborepo_ui::replay_logs(task, log_file),
        }
    }
}

/// Struct for displaying information about task
impl<W: Write> TaskOutput<W> {
    pub fn finish(self, use_error: bool, is_cache_hit: bool) -> std::io::Result<Option<Vec<u8>>> {
        match self {
            TaskOutput::Direct(client) => client.finish(use_error),
            TaskOutput::UI(client) if use_error => Ok(Some(client.failed())),
            TaskOutput::UI(client) => Ok(Some(client.succeeded(is_cache_hit))),
        }
    }

    pub fn stdout(&self) -> Either<OutputWriter<W>, TaskSender> {
        match self {
            TaskOutput::Direct(client) => Either::Left(client.stdout()),
            TaskOutput::UI(client) => Either::Right(client.clone()),
        }
    }

    pub fn stderr(&self) -> Either<OutputWriter<W>, TaskSender> {
        match self {
            TaskOutput::Direct(client) => Either::Left(client.stderr()),
            TaskOutput::UI(client) => Either::Right(client.clone()),
        }
    }

    pub fn task_logs(&self) -> Either<OutputWriter<W>, TaskSender> {
        match self {
            TaskOutput::Direct(client) => Either::Left(client.stdout()),
            TaskOutput::UI(client) => Either::Right(client.clone()),
        }
    }
}
