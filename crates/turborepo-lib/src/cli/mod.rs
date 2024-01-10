use std::{backtrace, backtrace::Backtrace, env, io, mem, process};

use camino::{Utf8Path, Utf8PathBuf};
use clap::{
    builder::NonEmptyStringValueParser, ArgAction, ArgGroup, CommandFactory, Parser, Subcommand,
    ValueEnum,
};
use clap_complete::{generate, Shell};
pub use error::Error;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};
use turbopath::AbsoluteSystemPathBuf;
use turborepo_api_client::AnonAPIClient;
use turborepo_repository::inference::{RepoMode, RepoState};
use turborepo_telemetry::{
    events::{
        command::{CodePath, CommandEventBuilder},
        generic::GenericEventBuilder,
        EventBuilder, EventType,
    },
    init_telemetry, TelemetryHandle,
};
use turborepo_ui::UI;

use crate::{
    commands::{
        bin, daemon, generate, info, link, login, logout, prune, telemetry, unlink, CommandBase,
    },
    get_version,
    shim::TurboState,
    tracing::TurboSubscriber,
    Payload,
};

mod error;

// Global turbo sets this environment variable to its cwd so that local
// turbo can use it for package inference.
pub const INVOCATION_DIR_ENV_VAR: &str = "TURBO_INVOCATION_DIR";

// Default value for the --cache-workers argument
const DEFAULT_NUM_WORKERS: u32 = 10;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ValueEnum)]
pub enum OutputLogsMode {
    #[serde(rename = "full")]
    Full,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "hash-only")]
    HashOnly,
    #[serde(rename = "new-only")]
    NewOnly,
    #[serde(rename = "errors-only")]
    ErrorsOnly,
}

impl Default for OutputLogsMode {
    fn default() -> Self {
        Self::Full
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, ValueEnum)]
pub enum LogOrder {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "stream")]
    Stream,
    #[serde(rename = "grouped")]
    Grouped,
}

impl Default for LogOrder {
    fn default() -> Self {
        Self::Auto
    }
}

// NOTE: These *must* be kept in sync with the `_dryRunJSONValue`
// and `_dryRunTextValue` constants in run.go.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, ValueEnum)]
pub enum DryRunMode {
    Text,
    Json,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, ValueEnum)]
pub enum EnvMode {
    #[default]
    Infer,
    Loose,
    Strict,
}

#[derive(Parser, Clone, Default, Debug, PartialEq, Serialize)]
#[clap(author, about = "The build system that makes ship happen", long_about = None)]
#[clap(disable_help_subcommand = true)]
#[clap(disable_version_flag = true)]
#[clap(arg_required_else_help = true)]
#[command(name = "turbo")]
pub struct Args {
    #[clap(long, global = true)]
    #[serde(skip)]
    pub version: bool,
    #[clap(long, global = true)]
    #[serde(skip)]
    /// Skip any attempts to infer which version of Turbo the project is
    /// configured to use
    pub skip_infer: bool,
    /// Disable the turbo update notification
    #[clap(long, global = true)]
    #[serde(skip)]
    pub no_update_notifier: bool,
    /// Override the endpoint for API calls
    #[clap(long, global = true, value_parser)]
    pub api: Option<String>,
    /// Force color usage in the terminal
    #[clap(long, global = true)]
    pub color: bool,
    /// Specify a file to save a cpu profile
    #[clap(long = "cpuprofile", global = true, value_parser)]
    pub cpu_profile: Option<String>,
    /// The directory in which to run turbo
    #[clap(long, global = true, value_parser)]
    pub cwd: Option<Utf8PathBuf>,
    /// Specify a file to save a pprof heap profile
    #[clap(long, global = true, value_parser)]
    pub heap: Option<String>,
    /// Override the login endpoint
    #[clap(long, global = true, value_parser)]
    pub login: Option<String>,
    /// Suppress color usage in the terminal
    #[clap(long, global = true)]
    pub no_color: bool,
    /// When enabled, turbo will precede HTTP requests with an OPTIONS request
    /// for authorization
    #[clap(long, global = true)]
    pub preflight: bool,
    /// Set a timeout for all HTTP requests.
    #[clap(long, value_name = "TIMEOUT", global = true, value_parser)]
    pub remote_cache_timeout: Option<u64>,
    /// Set the team slug for API calls
    #[clap(long, global = true, value_parser)]
    pub team: Option<String>,
    /// Set the auth token for API calls
    #[clap(long, global = true, value_parser)]
    pub token: Option<String>,
    /// Specify a file to save a pprof trace
    #[clap(long, global = true, value_parser)]
    pub trace: Option<String>,
    /// verbosity
    #[clap(flatten)]
    pub verbosity: Verbosity,
    /// Force a check for a new version of turbo
    #[clap(long, global = true, hide = true)]
    #[serde(skip)]
    pub check_for_update: bool,
    #[clap(long = "__test-run", global = true, hide = true)]
    pub test_run: bool,
    #[clap(flatten, next_help_heading = "Run Arguments")]
    // We don't serialize this because by the time we're calling
    // Go, we've moved it to the command field as a Command::Run
    #[serde(skip)]
    pub run_args: Option<RunArgs>,
    #[clap(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Parser, Serialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(into = "u8")]
pub struct Verbosity {
    #[clap(
        long = "verbosity",
        global = true,
        conflicts_with = "v",
        value_name = "COUNT"
    )]
    /// Verbosity level
    pub verbosity: Option<u8>,
    #[clap(
        short = 'v',
        action = clap::ArgAction::Count,
        global = true,
        hide = true,
        conflicts_with = "verbosity"
    )]
    pub v: u8,
}

impl From<Verbosity> for u8 {
    fn from(val: Verbosity) -> Self {
        let Verbosity { verbosity, v } = val;
        verbosity.unwrap_or(v)
    }
}

#[derive(Subcommand, Copy, Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "command")]
pub enum DaemonCommand {
    /// Restarts the turbo daemon
    Restart,
    /// Ensures that the turbo daemon is running
    Start,
    /// Reports the status of the turbo daemon
    Status {
        /// Pass --json to report status in JSON format
        #[clap(long)]
        json: bool,
    },
    /// Stops the turbo daemon
    Stop,
    /// Stops the turbo daemon if it is already running, and removes any stale
    /// daemon state
    Clean,
}

#[derive(Subcommand, Copy, Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "command")]
pub enum TelemetryCommand {
    /// Enables anonymous telemetry
    Enable,
    /// Disables anonymous telemetry
    Disable,
    /// Reports the status of telemetry
    Status,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, ValueEnum)]
pub enum LinkTarget {
    RemoteCache,
    Spaces,
}

impl Args {
    pub fn new() -> Self {
        // We always pass --single-package in from the shim.
        // We need to omit it, and then add it in for run.
        let arg_separator_position = env::args_os().position(|input_token| input_token == "--");

        let single_package_position =
            env::args_os().position(|input_token| input_token == "--single-package");

        let is_single_package = match (arg_separator_position, single_package_position) {
            (_, None) => false,
            (None, Some(_)) => true,
            (Some(arg_separator_position), Some(single_package_position)) => {
                single_package_position < arg_separator_position
            }
        };

        // Clap supports arbitrary iterators as input.
        // We can remove all instances of --single-package
        let single_package_free = std::env::args_os()
            .enumerate()
            .filter(|(index, input_token)| {
                arg_separator_position
                    .is_some_and(|arg_separator_position| index > &arg_separator_position)
                    || input_token != "--single-package"
            })
            .map(|(_, input_token)| input_token);

        let mut clap_args = match Args::try_parse_from(single_package_free) {
            Ok(mut args) => {
                // And then only add them back in when we're in `run`.
                // The value can appear in two places in the struct.
                // We defensively attempt to set both.
                if let Some(ref mut run_args) = args.run_args {
                    run_args.single_package = is_single_package
                }

                if let Some(Command::Run(ref mut run_args)) = args.command {
                    run_args.single_package = is_single_package;
                }

                args
            }
            // Don't use error logger when displaying help text
            Err(e)
                if matches!(
                    e.kind(),
                    clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                ) =>
            {
                let _ = e.print();
                process::exit(1);
            }
            Err(e) if e.use_stderr() => {
                let err_str = e.to_string();
                // A cleaner solution would be to implement our own clap::error::ErrorFormatter
                // but that would require copying the default formatter just to remove this
                // line: https://docs.rs/clap/latest/src/clap/error/format.rs.html#100
                error!(
                    "{}",
                    err_str.strip_prefix("error: ").unwrap_or(err_str.as_str())
                );
                process::exit(1);
            }
            // If the clap error shouldn't be printed to stderr it indicates help text
            Err(e) => {
                let _ = e.print();
                process::exit(0);
            }
        };
        // We have to override the --version flag because we use `get_version`
        // instead of a hard-coded version or the crate version
        if clap_args.version {
            println!("{}", get_version());
            process::exit(0);
        }

        if env::var("TEST_RUN").is_ok() {
            clap_args.test_run = true;
        }

        clap_args
    }

    pub fn get_tasks(&self) -> &[String] {
        match &self.command {
            Some(Command::Run(box RunArgs { tasks, .. })) => tasks,
            _ => self
                .run_args
                .as_ref()
                .map(|run_args| run_args.tasks.as_slice())
                .unwrap_or(&[]),
        }
    }

    pub fn track(&self, telemetry: &GenericEventBuilder) {
        // track usage only
        if self.skip_infer {
            telemetry.track_arg_usage("skip-infer", true);
        }
        if self.no_update_notifier {
            telemetry.track_arg_usage("no-update-notifier", true);
        }
        if self.api.is_some() {
            telemetry.track_arg_usage("api", true);
        }
        if self.color {
            telemetry.track_arg_usage("color", true);
        }
        if self.cpu_profile.is_some() {
            telemetry.track_arg_usage("cpuprofile", true);
        }
        if self.cwd.is_some() {
            telemetry.track_arg_usage("cwd", true);
        }
        if self.heap.is_some() {
            telemetry.track_arg_usage("heap", true);
        }
        if self.login.is_some() {
            telemetry.track_arg_usage("login", true);
        }
        if self.no_color {
            telemetry.track_arg_usage("no-color", true);
        }
        if self.preflight {
            telemetry.track_arg_usage("preflight", true);
        }
        if self.team.is_some() {
            telemetry.track_arg_usage("team", true);
        }
        if self.token.is_some() {
            telemetry.track_arg_usage("token", true);
        }
        if self.trace.is_some() {
            telemetry.track_arg_usage("trace", true);
        }

        // track values
        if let Some(remote_cache_timeout) = self.remote_cache_timeout {
            telemetry.track_arg_value(
                "remote-cache-timeout",
                remote_cache_timeout,
                turborepo_telemetry::events::EventType::NonSensitive,
            );
        }
        if self.verbosity.v > 0 {
            telemetry.track_arg_value(
                "v",
                self.verbosity.v,
                turborepo_telemetry::events::EventType::NonSensitive,
            );
        }
        if let Some(verbosity) = self.verbosity.verbosity {
            telemetry.track_arg_value(
                "verbosity",
                verbosity,
                turborepo_telemetry::events::EventType::NonSensitive,
            );
        }
    }
}

/// Defines the subcommands for CLI. NOTE: If we change the commands in Go,
/// we must change these as well to avoid accidentally passing the
/// --single-package flag into non-build commands.
#[derive(Subcommand, Clone, Debug, Serialize, PartialEq)]
pub enum Command {
    // NOTE: Empty variants still have an empty struct attached so that serde serializes
    // them as `{ "Bin": {} }` instead of as `"Bin"`.
    /// Get the path to the Turbo binary
    Bin {},
    /// Generate the autocompletion script for the specified shell
    #[serde(skip)]
    Completion { shell: Shell },
    /// Runs the Turborepo background daemon
    Daemon {
        /// Set the idle timeout for turbod
        #[clap(long, default_value_t = String::from("4h0m0s"))]
        idle_time: String,
        #[clap(subcommand)]
        #[serde(flatten)]
        command: Option<DaemonCommand>,
    },
    /// Generate a new app / package
    #[clap(aliases = ["g", "gen"])]
    Generate {
        #[serde(skip)]
        #[clap(long, default_value_t = String::from("latest"), hide = true)]
        tag: String,
        /// The name of the generator to run
        generator_name: Option<String>,
        /// Generator configuration file
        #[clap(short = 'c', long)]
        config: Option<String>,
        /// The root of your repository (default: directory with root
        /// turbo.json)
        #[clap(short = 'r', long)]
        root: Option<String>,
        /// Answers passed directly to generator
        #[clap(short = 'a', long, num_args = 1..)]
        args: Vec<String>,

        #[clap(subcommand)]
        #[serde(skip)]
        command: Option<Box<GenerateCommand>>,
    },
    // TODO:[telemetry] Unhide this in `1.12`
    /// Enable or disable anonymous telemetry
    #[clap(hide = true)]
    Telemetry {
        #[clap(subcommand)]
        #[serde(flatten)]
        command: Option<TelemetryCommand>,
    },
    #[clap(hide = true)]
    Info {
        workspace: Option<String>,
        // We output turbo info as json. Currently just for internal testing
        #[clap(long)]
        json: bool,
    },
    /// Link your local directory to a Vercel organization and enable remote
    /// caching.
    Link {
        /// Do not create or modify .gitignore (default false)
        #[clap(long)]
        no_gitignore: bool,

        /// Specify what should be linked (default "remote cache")
        #[clap(long, value_enum, default_value_t = LinkTarget::RemoteCache)]
        target: LinkTarget,
    },
    /// Login to your Vercel account
    Login {
        #[clap(long = "sso-team")]
        sso_team: Option<String>,
    },
    /// Logout to your Vercel account
    Logout {},
    /// Prepare a subset of your monorepo.
    Prune {
        #[clap(hide = true, long)]
        scope: Option<Vec<String>>,
        /// Workspaces that should be included in the subset
        #[clap(
            required_unless_present("scope"),
            conflicts_with("scope"),
            value_name = "SCOPE"
        )]
        scope_arg: Option<Vec<String>>,
        #[clap(long)]
        docker: bool,
        #[clap(long = "out-dir", default_value_t = String::from(prune::DEFAULT_OUTPUT_DIR), value_parser)]
        output_dir: String,
    },

    /// Run tasks across projects in your monorepo
    ///
    /// By default, turbo executes tasks in topological order (i.e.
    /// dependencies first) and then caches the results. Re-running commands for
    /// tasks already in the cache will skip re-execution and immediately move
    /// artifacts from the cache into the correct output folders (as if the task
    /// occurred again).
    ///
    /// Arguments passed after '--' will be passed through to the named tasks.
    Run(Box<RunArgs>),
    /// Unlink the current directory from your Vercel organization and disable
    /// Remote Caching
    Unlink {
        /// Specify what should be unlinked (default "remote cache")
        #[clap(long, value_enum, default_value_t = LinkTarget::RemoteCache)]
        target: LinkTarget,
    },
}

#[derive(Parser, Clone, Debug, Default, Serialize, PartialEq)]
pub struct GenerateWorkspaceArgs {
    /// Name for the new workspace
    #[clap(short = 'n', long)]
    pub name: Option<String>,
    /// Generate an empty workspace
    #[clap(short = 'b', long, conflicts_with = "copy", default_value_t = true)]
    pub empty: bool,
    /// Generate a workspace using an existing workspace as a template. Can be
    /// the name of a local workspace within your monorepo, or a fully
    /// qualified GitHub URL with any branch and/or subdirectory
    #[clap(short = 'c', long, conflicts_with = "empty", num_args = 0..=1, default_missing_value = "")]
    pub copy: Option<String>,
    /// Where the new workspace should be created
    #[clap(short = 'd', long)]
    pub destination: Option<String>,
    /// The type of workspace to create
    #[clap(short = 't', long)]
    pub r#type: Option<String>,
    /// The root of your repository (default: directory with root turbo.json)
    #[clap(short = 'r', long)]
    pub root: Option<String>,
    /// In a rare case, your GitHub URL might contain a branch name with a slash
    /// (e.g. bug/fix-1) and the path to the example (e.g. foo/bar). In this
    /// case, you must specify the path to the example separately:
    /// --example-path foo/bar
    #[clap(short = 'p', long)]
    pub example_path: Option<String>,
    /// Do not filter available dependencies by the workspace type
    #[clap(long, default_value_t = false)]
    pub show_all_dependencies: bool,
}

#[derive(Parser, Clone, Debug, Default, Serialize, PartialEq)]
pub struct GeneratorCustomArgs {
    /// The name of the generator to run
    generator_name: Option<String>,
    /// Generator configuration file
    #[clap(short = 'c', long)]
    config: Option<String>,
    /// The root of your repository (default: directory with root
    /// turbo.json)
    #[clap(short = 'r', long)]
    root: Option<String>,
    /// Answers passed directly to generator
    #[clap(short = 'a', long, value_delimiter = ' ', num_args = 1..)]
    args: Vec<String>,
}

#[derive(Subcommand, Clone, Debug, Serialize, PartialEq)]
pub enum GenerateCommand {
    /// Add a new package or app to your project
    #[clap(name = "workspace", alias = "w")]
    Workspace(GenerateWorkspaceArgs),
    #[clap(name = "run", alias = "r")]
    Run(GeneratorCustomArgs),
}

#[derive(Parser, Clone, Debug, Default, Serialize, PartialEq)]
#[command(groups = [
    ArgGroup::new("daemon-group").multiple(false).required(false),
    ArgGroup::new("scope-filter-group").multiple(true).required(false),
])]
pub struct RunArgs {
    /// Override the filesystem cache directory.
    #[clap(long)]
    pub cache_dir: Option<Utf8PathBuf>,
    /// Set the number of concurrent cache operations (default 10)
    #[clap(long, default_value_t = DEFAULT_NUM_WORKERS)]
    pub cache_workers: u32,
    /// Limit the concurrency of task execution. Use 1 for serial (i.e.
    /// one-at-a-time) execution.
    #[clap(long)]
    pub concurrency: Option<String>,
    /// Continue execution even if a task exits with an error or non-zero
    /// exit code. The default behavior is to bail
    #[clap(long = "continue")]
    pub continue_execution: bool,
    #[clap(alias = "dry", long = "dry-run", num_args = 0..=1, default_missing_value = "text")]
    pub dry_run: Option<DryRunMode>,
    /// Fallback to use Go for task execution
    #[serde(skip)]
    #[clap(long, conflicts_with = "remote_cache_read_only")]
    pub go_fallback: bool,
    /// Run turbo in single-package mode
    #[clap(long)]
    pub single_package: bool,
    /// Ignore the existing cache (to force execution)
    #[clap(long, env = "TURBO_FORCE", default_missing_value = "true")]
    pub force: Option<Option<bool>>,
    /// Specify whether or not to do framework inference for tasks
    #[clap(long, value_name = "BOOL", action = ArgAction::Set, default_value = "true", default_missing_value = "true", num_args = 0..=1)]
    pub framework_inference: bool,
    /// Specify glob of global filesystem dependencies to be hashed. Useful
    /// for .env and files
    #[clap(long = "global-deps", action = ArgAction::Append)]
    pub global_deps: Vec<String>,
    /// Generate a graph of the task execution and output to a file when a
    /// filename is specified (.svg, .png, .jpg, .pdf, .json,
    /// .html). Outputs dot graph to stdout when if no filename is provided
    #[clap(long, num_args = 0..=1, default_missing_value = "")]
    pub graph: Option<String>,
    /// Environment variable mode.
    /// Use "loose" to pass the entire existing environment.
    /// Use "strict" to use an allowlist specified in turbo.json.
    /// Use "infer" to defer to existence of "passThroughEnv" or
    /// "globalPassThroughEnv" in turbo.json. (default infer)
    #[clap(long = "env-mode", default_value = "infer", num_args = 0..=1, default_missing_value = "infer")]
    pub env_mode: EnvMode,

    /// Use the given selector to specify package(s) to act as
    /// entry points. The syntax mirrors pnpm's syntax, and
    /// additional documentation and examples can be found in
    /// turbo's documentation https://turbo.build/repo/docs/reference/command-line-reference/run#--filter
    #[clap(short = 'F', long, group = "scope-filter-group")]
    pub filter: Vec<String>,

    /// DEPRECATED: Specify package(s) to act as entry
    /// points for task execution. Supports globs.
    #[clap(long, group = "scope-filter-group")]
    pub scope: Vec<String>,

    //  ignore filters out files from scope and filter, so we require it here
    // -----------------------
    /// Files to ignore when calculating changed files from '--filter'.
    /// Supports globs.
    #[clap(long, requires = "scope-filter-group")]
    pub ignore: Vec<String>,

    //  since only works with scope, so we require it here
    // -----------------------
    /// DEPRECATED: Limit/Set scope to changed packages
    /// since a mergebase. This uses the git diff ${target_branch}...
    /// mechanism to identify which packages have changed.
    #[clap(long, requires = "scope")]
    pub since: Option<String>,

    //  include_dependencies only works with scope, so we require it here
    // -----------------------
    /// DEPRECATED: Include the dependencies of tasks in execution.
    #[clap(long, requires = "scope")]
    pub include_dependencies: bool,

    //  no_deps only works with scope, so we require it here
    // -----------------------
    /// DEPRECATED: Exclude dependent task consumers from execution.
    #[clap(long, requires = "scope")]
    pub no_deps: bool,

    /// Avoid saving task results to the cache. Useful for development/watch
    /// tasks.
    #[clap(long)]
    pub no_cache: bool,

    // clap does not have negation flags such as --daemon and --no-daemon
    // so we need to use a group to enforce that only one of them is set.
    // we set the long name as [no-]daemon with an alias of daemon such
    // that we can merge the help text together for both flags
    // -----------------------
    /// Force turbo to either use or not use the local daemon. If unset
    /// turbo will use the default detection logic.
    #[clap(long = "[no-]daemon", alias = "daemon", group = "daemon-group")]
    daemon: bool,

    #[clap(long, group = "daemon-group", hide = true)]
    no_daemon: bool,

    /// Set type of process output logging. Use "full" to show
    /// all output. Use "hash-only" to show only turbo-computed
    /// task hashes. Use "new-only" to show only new output with
    /// only hashes for cached tasks. Use "none" to hide process
    /// output. (default full)
    #[clap(long, value_enum)]
    pub output_logs: Option<OutputLogsMode>,

    /// Set type of task output order. Use "stream" to show
    /// output as soon as it is available. Use "grouped" to
    /// show output when a command has finished execution. Use "auto" to let
    /// turbo decide based on its own heuristics. (default auto)
    #[clap(long, env = "TURBO_LOG_ORDER", value_enum, default_value_t = LogOrder::Auto)]
    pub log_order: LogOrder,
    /// Only executes the tasks specified, does not execute parent tasks.
    #[clap(long)]
    pub only: bool,
    /// Execute all tasks in parallel.
    #[clap(long)]
    pub parallel: bool,
    #[clap(long, hide = true)]
    pub pkg_inference_root: Option<String>,
    /// File to write turbo's performance profile output into.
    /// You can load the file up in chrome://tracing to see
    /// which parts of your build were slow.
    #[clap(long, value_parser=NonEmptyStringValueParser::new(), conflicts_with = "anon_profile")]
    pub profile: Option<String>,
    /// File to write turbo's performance profile output into.
    /// All identifying data omitted from the profile.
    #[serde(skip)]
    #[clap(long, value_parser=NonEmptyStringValueParser::new(), conflicts_with = "profile")]
    pub anon_profile: Option<String>,
    /// Ignore the local filesystem cache for all tasks. Only
    /// allow reading and caching artifacts using the remote cache.
    #[clap(long, env = "TURBO_REMOTE_ONLY", value_name = "BOOL", action = ArgAction::Set, default_value = "false", default_missing_value = "true", num_args = 0..=1)]
    pub remote_only: bool,
    /// Treat remote cache as read only
    #[clap(long, env = "TURBO_REMOTE_CACHE_READ_ONLY", value_name = "BOOL", action = ArgAction::Set, default_value = "false", default_missing_value = "true", num_args = 0..=1)]
    #[serde(skip)]
    pub remote_cache_read_only: bool,
    /// Generate a summary of the turbo run
    #[clap(long, env = "TURBO_RUN_SUMMARY", default_missing_value = "true")]
    pub summarize: Option<Option<bool>>,

    /// Use "none" to remove prefixes from task logs. Use "task" to get task id
    /// prefixing. Use "auto" to let turbo decide how to prefix the logs
    /// based on the execution environment. In most cases this will be the same
    /// as "task". Note that tasks running in parallel interleave their
    /// logs, so removing prefixes can make it difficult to associate logs
    /// with tasks. Use --log-order=grouped to prevent interleaving. (default
    /// auto)
    #[clap(long, value_enum, default_value_t = LogPrefix::Auto)]
    pub log_prefix: LogPrefix,

    // NOTE: The following two are hidden because clap displays them in the help text incorrectly:
    // > Usage: turbo [OPTIONS] [TASKS]... [-- <FORWARDED_ARGS>...] [COMMAND]
    #[clap(hide = true)]
    pub tasks: Vec<String>,
    #[clap(last = true, hide = true)]
    pub pass_through_args: Vec<String>,

    // Pass a string to enable posting Run Summaries to Vercel
    #[clap(long, hide = true)]
    pub experimental_space_id: Option<String>,
}

impl RunArgs {
    /// Some(true) means force the daemon
    /// Some(false) means force no daemon
    /// None means use the default detection
    pub fn daemon(&self) -> Option<bool> {
        match (self.daemon, self.no_daemon) {
            (true, false) => Some(true),
            (false, true) => Some(false),
            (false, false) => None,
            (true, true) => unreachable!(), // guaranteed by mutually exclusive `ArgGroup`
        }
    }

    pub fn profile_file_and_include_args(&self) -> Option<(&str, bool)> {
        match (self.profile.as_deref(), self.anon_profile.as_deref()) {
            (Some(file), None) => Some((file, true)),
            (None, Some(file)) => Some((file, false)),
            (Some(_), Some(_)) => unreachable!(),
            (None, None) => None,
        }
    }

    pub fn track(&self, telemetry: &CommandEventBuilder) {
        // track usage
        if self.cache_dir.is_some() {
            telemetry.track_arg_usage("cache-dir", true);
        }
        if self.continue_execution {
            telemetry.track_arg_usage("continue", true);
        }
        if self.go_fallback {
            telemetry.track_arg_usage("go-fallback", true);
        }
        if self.single_package {
            telemetry.track_arg_usage("single-package", true);
        }
        if self.force.is_some() {
            telemetry.track_arg_usage("force", true);
        }
        // framework_inference defaults to true, so we only track it if it's false
        if !self.framework_inference {
            telemetry.track_arg_usage("framework-inference", true);
        }
        if self.include_dependencies {
            telemetry.track_arg_usage("include-dependencies", true);
        }
        if self.no_deps {
            telemetry.track_arg_usage("no-deps", true);
        }
        if self.no_cache {
            telemetry.track_arg_usage("no-cache", true);
        }
        if self.daemon {
            telemetry.track_arg_usage("daemon", true);
        }

        if self.since.is_some() {
            telemetry.track_arg_usage("since", true);
        }

        if self.include_dependencies {
            telemetry.track_arg_usage("include-dependencies", true);
        }

        if self.no_deps {
            telemetry.track_arg_usage("no-deps", true);
        }

        if self.no_cache {
            telemetry.track_arg_usage("no-cache", true);
        }

        if self.daemon {
            telemetry.track_arg_usage("daemon", true);
        }

        if self.no_daemon {
            telemetry.track_arg_usage("no_daemon", true);
        }

        if self.only {
            telemetry.track_arg_usage("only", true);
        }

        if self.parallel {
            telemetry.track_arg_usage("parallel", true);
        }

        if self.pkg_inference_root.is_some() {
            telemetry.track_arg_usage("pkg-inference-root", true);
        }

        if self.profile.is_some() {
            telemetry.track_arg_usage("profile", true);
        }

        if self.anon_profile.is_some() {
            telemetry.track_arg_usage("anon-profile", true);
        }

        if self.remote_only {
            telemetry.track_arg_usage("remote-only", true);
        }

        if self.remote_cache_read_only {
            telemetry.track_arg_usage("remote-cache-read-only", true);
        }

        if self.summarize.is_some() {
            telemetry.track_arg_usage("summarize", true);
        }

        if self.experimental_space_id.is_some() {
            telemetry.track_arg_usage("experimental-space-id", true);
        }

        // track values
        if let Some(dry_run) = &self.dry_run {
            telemetry.track_arg_value(
                "dry-run",
                match dry_run {
                    DryRunMode::Text => "text",
                    DryRunMode::Json => "json",
                },
                EventType::NonSensitive,
            );
        }

        if self.cache_workers != DEFAULT_NUM_WORKERS {
            telemetry.track_arg_value("cache-workers", self.cache_workers, EventType::NonSensitive);
        }

        if let Some(concurrency) = &self.concurrency {
            telemetry.track_arg_value("concurrency", concurrency, EventType::NonSensitive);
        }

        if !self.global_deps.is_empty() {
            telemetry.track_arg_value("global-deps", self.cache_workers, EventType::NonSensitive);
        }

        if let Some(graph) = &self.graph {
            telemetry.track_arg_value("graph", graph, EventType::NonSensitive);
        }

        if self.env_mode != EnvMode::default() {
            telemetry.track_arg_value(
                "env-mode",
                match self.env_mode {
                    EnvMode::Infer => "infer",
                    EnvMode::Loose => "loose",
                    EnvMode::Strict => "strict",
                },
                EventType::NonSensitive,
            );
        }

        if let Some(output_logs) = &self.output_logs {
            telemetry.track_arg_value(
                "output-logs",
                match output_logs {
                    OutputLogsMode::Full => "full",
                    OutputLogsMode::None => "none",
                    OutputLogsMode::HashOnly => "hash-only",
                    OutputLogsMode::NewOnly => "new-only",
                    OutputLogsMode::ErrorsOnly => "errors-only",
                },
                EventType::NonSensitive,
            );
        }

        if self.log_order != LogOrder::default() {
            telemetry.track_arg_value(
                "log-order",
                match self.log_order {
                    LogOrder::Auto => "auto",
                    LogOrder::Stream => "stream",
                    LogOrder::Grouped => "grouped",
                },
                EventType::NonSensitive,
            );
        }

        if self.log_prefix != LogPrefix::default() {
            telemetry.track_arg_value(
                "log-prefix",
                match self.log_prefix {
                    LogPrefix::Auto => "auto",
                    LogPrefix::None => "none",
                    LogPrefix::Task => "task",
                },
                EventType::NonSensitive,
            );
        }

        // track sizes
        if !self.filter.is_empty() {
            telemetry.track_arg_value("filter", self.filter.len(), EventType::NonSensitive);
        }

        if !self.scope.is_empty() {
            telemetry.track_arg_value("scope", self.scope.len(), EventType::NonSensitive);
        }

        if !self.ignore.is_empty() {
            telemetry.track_arg_value("ignore", self.ignore.len(), EventType::NonSensitive);
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Serialize)]
pub enum LogPrefix {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "task")]
    Task,
}

impl Default for LogPrefix {
    fn default() -> Self {
        Self::Auto
    }
}

/// Runs the CLI by parsing arguments with clap, then either calling Rust code
/// directly or returning a payload for the Go code to use.
///
/// Scenarios:
/// 1. inference failed, we're running this global turbo. no repo state
/// 2. --skip-infer was passed, assume we're local turbo and run. no repo state
/// 3. There is no local turbo, we're running the global one. repo state exists
/// 4. turbo binary path is set, and it's this one. repo state exists
///
/// # Arguments
///
/// * `repo_state`: If we have done repository inference and NOT executed
/// local turbo, such as in the case where `TURBO_BINARY_PATH` is set,
/// we use it here to modify clap's arguments.
/// * `logger`: The logger to use for the run.
/// * `ui`: The UI to use for the run.
///
/// returns: Result<Payload, Error>
#[tokio::main]
pub async fn run(
    repo_state: Option<RepoState>,
    #[allow(unused_variables)] logger: &TurboSubscriber,
    ui: UI,
) -> Result<Payload, Error> {
    let mut cli_args = Args::new();
    let version = get_version();

    // track telemetry handle to close at the end of the run
    let mut telemetry_handle: Option<TelemetryHandle> = None;
    // TODO:[telemetry] Remove this check in `1.12`
    if turborepo_telemetry::config::is_telemetry_internal_test() {
        // initialize telemetry
        match AnonAPIClient::new("https://telemetry.vercel.com", 250, version) {
            Ok(anonymous_api_client) => {
                let handle = init_telemetry(anonymous_api_client, ui);
                match handle {
                    Ok(h) => telemetry_handle = Some(h),
                    Err(error) => {
                        debug!("failed to start telemetry: {:?}", error)
                    }
                }
            }
            Err(error) => {
                debug!("Failed to create AnonAPIClient: {:?}", error);
            }
        }
    }

    // If there is no command, we set the command to `Command::Run` with
    // `self.parsed_args.run_args` as arguments.
    let mut command = if let Some(command) = mem::take(&mut cli_args.command) {
        command
    } else {
        let run_args = mem::take(&mut cli_args.run_args)
            .ok_or_else(|| Error::NoCommand(Backtrace::capture()))?;
        if run_args.tasks.is_empty() {
            let mut cmd = <Args as CommandFactory>::command();
            let _ = cmd.print_help();
            process::exit(1);
        }

        Command::Run(Box::new(run_args))
    };

    // Set some run flags if we have the data and are executing a Run
    if let Command::Run(run_args) = &mut command {
        // Don't overwrite the flag if it's already been set for whatever reason
        run_args.single_package = run_args.single_package
            || repo_state
                .as_ref()
                .map(|repo_state| matches!(repo_state.mode, RepoMode::SinglePackage))
                .unwrap_or(false);
        // If this is a run command, and we know the actual invocation path, set the
        // inference root, as long as the user hasn't overridden the cwd
        if cli_args.cwd.is_none() {
            if let Ok(invocation_dir) = env::var(INVOCATION_DIR_ENV_VAR) {
                // TODO: this calculation can probably be wrapped into the path library
                // and made a little more robust or clear
                let invocation_path = Utf8Path::new(&invocation_dir);

                // If repo state doesn't exist, we're either local turbo running at the root
                // (cwd), or inference failed.
                // If repo state does exist, we're global turbo, and want to calculate
                // package inference based on the repo root
                let this_dir = AbsoluteSystemPathBuf::cwd()?;
                let repo_root = repo_state.as_ref().map_or(&this_dir, |r| &r.root);
                if let Ok(relative_path) = invocation_path.strip_prefix(repo_root) {
                    if !relative_path.as_str().is_empty() {
                        debug!("pkg_inference_root set to \"{}\"", relative_path);
                        run_args.pkg_inference_root = Some(relative_path.to_string());
                    }
                }
            } else {
                debug!("{} not set", INVOCATION_DIR_ENV_VAR);
            }
        }
    }

    // TODO: make better use of RepoState, here and below. We've already inferred
    // the repo root, we don't need to calculate it again, along with package
    // manager inference.
    let cwd = repo_state
        .as_ref()
        .map(|state| state.root.as_path())
        .or(cli_args.cwd.as_deref());

    let repo_root = if let Some(cwd) = cwd {
        AbsoluteSystemPathBuf::from_cwd(cwd)?
    } else {
        AbsoluteSystemPathBuf::cwd()?
    };

    cli_args.command = Some(command);
    cli_args.cwd = Some(repo_root.as_path().to_owned());

    let root_telemetry = GenericEventBuilder::new();
    root_telemetry.track_start();

    // track system info
    root_telemetry.track_platform(TurboState::platform_name());
    root_telemetry.track_version(TurboState::version());
    root_telemetry.track_cpus(num_cpus::get());
    // track args
    cli_args.track(&root_telemetry);

    let cli_result = match cli_args.command.as_ref().unwrap() {
        Command::Bin { .. } => {
            CommandEventBuilder::new("bin")
                .with_parent(&root_telemetry)
                .track_call();
            bin::run()?;

            Ok(Payload::Rust(Ok(0)))
        }
        #[allow(unused_variables)]
        Command::Daemon { command, idle_time } => {
            CommandEventBuilder::new("daemon")
                .with_parent(&root_telemetry)
                .track_call();
            let base = CommandBase::new(cli_args.clone(), repo_root, version, ui);

            match command {
                Some(command) => daemon::daemon_client(command, &base).await,
                #[cfg(not(feature = "go-daemon"))]
                None => daemon::daemon_server(&base, idle_time, logger).await,
                #[cfg(feature = "go-daemon")]
                None => {
                    return Ok(Payload::Go(Box::new(base)));
                }
            }?;

            Ok(Payload::Rust(Ok(0)))
        }
        Command::Generate {
            tag,
            generator_name,
            config,
            root,
            args,
            command,
        } => {
            let event = CommandEventBuilder::new("generate").with_parent(&root_telemetry);
            event.track_call();
            // build GeneratorCustomArgs struct
            let args = GeneratorCustomArgs {
                generator_name: generator_name.clone(),
                config: config.clone(),
                root: root.clone(),
                args: args.clone(),
            };
            let child_event = event.child();
            generate::run(tag, command, &args, child_event)?;
            Ok(Payload::Rust(Ok(0)))
        }
        Command::Telemetry { command } => {
            let event = CommandEventBuilder::new("telemetry").with_parent(&root_telemetry);
            event.track_call();
            let mut base = CommandBase::new(cli_args.clone(), repo_root, version, ui);
            let child_event = event.child();
            telemetry::configure(command, &mut base, child_event);
            Ok(Payload::Rust(Ok(0)))
        }
        Command::Info { workspace, json } => {
            CommandEventBuilder::new("info")
                .with_parent(&root_telemetry)
                .track_call();
            let json = *json;
            let workspace = workspace.clone();
            let mut base = CommandBase::new(cli_args, repo_root, version, ui);
            info::run(&mut base, workspace.as_deref(), json).await?;

            Ok(Payload::Rust(Ok(0)))
        }
        Command::Link {
            no_gitignore,
            target,
        } => {
            CommandEventBuilder::new("link")
                .with_parent(&root_telemetry)
                .track_call();
            if cli_args.test_run {
                println!("Link test run successful");
                return Ok(Payload::Rust(Ok(0)));
            }

            let modify_gitignore = !*no_gitignore;
            let to = *target;
            let mut base = CommandBase::new(cli_args, repo_root, version, ui);

            if let Err(err) = link::link(&mut base, modify_gitignore, to).await {
                error!("error: {}", err.to_string())
            }

            Ok(Payload::Rust(Ok(0)))
        }
        Command::Logout { .. } => {
            let event = CommandEventBuilder::new("logout").with_parent(&root_telemetry);
            event.track_call();
            let mut base = CommandBase::new(cli_args, repo_root, version, ui);

            let event_child = event.child();
            logout::logout(&mut base, event_child).await?;

            Ok(Payload::Rust(Ok(0)))
        }
        Command::Login { sso_team } => {
            let event = CommandEventBuilder::new("login").with_parent(&root_telemetry);
            event.track_call();
            if cli_args.test_run {
                println!("Login test run successful");
                return Ok(Payload::Rust(Ok(0)));
            }

            let sso_team = sso_team.clone();

            let mut base = CommandBase::new(cli_args, repo_root, version, ui);
            let event_child = event.child();

            if let Some(sso_team) = sso_team {
                login::sso_login(&mut base, &sso_team, event_child).await?;
            } else {
                login::login(&mut base, event_child).await?;
            }

            Ok(Payload::Rust(Ok(0)))
        }
        Command::Unlink { target } => {
            CommandEventBuilder::new("unlink")
                .with_parent(&root_telemetry)
                .track_call();
            if cli_args.test_run {
                println!("Unlink test run successful");
                return Ok(Payload::Rust(Ok(0)));
            }

            let from = *target;
            let mut base = CommandBase::new(cli_args, repo_root, version, ui);

            unlink::unlink(&mut base, from)?;

            Ok(Payload::Rust(Ok(0)))
        }
        Command::Run(args) => {
            let event = CommandEventBuilder::new("run").with_parent(&root_telemetry);
            event.track_call();
            // in the case of enabling the run stub, we want to be able to opt-in
            // to the rust codepath for running turbo
            if args.tasks.is_empty() {
                return Err(Error::NoTasks(backtrace::Backtrace::capture()));
            }

            if let Some((file_path, include_args)) = args.profile_file_and_include_args() {
                // TODO: Do we want to handle the result / error?
                let _ = logger.enable_chrome_tracing(file_path, include_args);
            }
            let base = CommandBase::new(cli_args.clone(), repo_root, version, ui);

            let should_use_go = args.go_fallback
                || env::var("EXPERIMENTAL_RUST_CODEPATH").as_deref() == Ok("false");

            args.track(&event);
            if should_use_go {
                event.track_run_code_path(CodePath::Go);
                // we have to clear the telemetry queue before we hand off to go
                if telemetry_handle.is_some() {
                    let handle = telemetry_handle.take().unwrap();
                    handle.close_with_timeout().await;
                }
                Ok(Payload::Go(Box::new(base)))
            } else {
                use crate::commands::run;
                event.track_run_code_path(CodePath::Rust);
                let exit_code = run::run(base, event).await?;
                Ok(Payload::Rust(Ok(exit_code)))
            }
        }
        Command::Prune {
            scope,
            scope_arg,
            docker,
            output_dir,
        } => {
            let event = CommandEventBuilder::new("prune").with_parent(&root_telemetry);
            event.track_call();
            let scope = scope_arg
                .as_ref()
                .or(scope.as_ref())
                .cloned()
                .unwrap_or_default();
            let docker = *docker;
            let output_dir = output_dir.clone();
            let base = CommandBase::new(cli_args, repo_root, version, ui);
            let event_child = event.child();
            prune::prune(&base, &scope, docker, &output_dir, event_child).await?;
            Ok(Payload::Rust(Ok(0)))
        }
        Command::Completion { shell } => {
            CommandEventBuilder::new("completion")
                .with_parent(&root_telemetry)
                .track_call();
            generate(*shell, &mut Args::command(), "turbo", &mut io::stdout());
            Ok(Payload::Rust(Ok(0)))
        }
    };

    if cli_result.is_err() {
        root_telemetry.track_failure();
    } else {
        root_telemetry.track_success();
    }
    root_telemetry.track_end();
    match telemetry_handle {
        Some(handle) => handle.close_with_timeout().await,
        None => debug!("Skipping telemetry close - not initialized"),
    }

    cli_result
}

#[cfg(test)]
mod test {
    use std::assert_matches::assert_matches;

    use camino::Utf8PathBuf;
    use clap::Parser;
    use itertools::Itertools;
    use pretty_assertions::assert_eq;

    struct CommandTestCase {
        command: &'static str,
        command_args: Vec<Vec<&'static str>>,
        global_args: Vec<Vec<&'static str>>,
        expected_output: Args,
    }

    fn get_default_run_args() -> RunArgs {
        RunArgs {
            cache_workers: 10,
            output_logs: None,
            remote_only: false,
            framework_inference: true,
            ..RunArgs::default()
        }
    }

    impl CommandTestCase {
        fn test(&self) {
            let permutations = self.create_all_arg_permutations();
            for command in permutations {
                assert_eq!(Args::try_parse_from(command).unwrap(), self.expected_output)
            }
        }

        fn create_all_arg_permutations(&self) -> Vec<Vec<&'static str>> {
            let mut permutations = Vec::new();
            let mut global_args = vec![vec![self.command]];
            global_args.extend(self.global_args.clone());
            let global_args_len = global_args.len();
            let command_args_len = self.command_args.len();

            // Iterate through all the different permutations of args
            for global_args_permutation in global_args.into_iter().permutations(global_args_len) {
                let command_args = self.command_args.clone();
                for command_args_permutation in
                    command_args.into_iter().permutations(command_args_len)
                {
                    let mut command = vec![vec!["turbo"]];
                    command.extend(global_args_permutation.clone());
                    command.extend(command_args_permutation);
                    permutations.push(command.into_iter().flatten().collect())
                }
            }

            permutations
        }
    }

    use anyhow::Result;

    use crate::cli::{
        Args, Command, DryRunMode, EnvMode, LogOrder, LogPrefix, OutputLogsMode, RunArgs, Verbosity,
    };

    #[test_case::test_case(
        &["turbo", "run", "build"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                ..get_default_run_args()
            }))),
            ..Args::default()
        } ;
        "default case"
    )]
    #[test_case::test_case(
        &["turbo", "run", "build"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                framework_inference: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
        } ;
        "framework_inference: default to true"
    )]
    #[test_case::test_case(
		&["turbo", "run", "build", "--framework-inference"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                framework_inference: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
        } ;
        "framework_inference: flag only"
    )]
    #[test_case::test_case(
		&["turbo", "run", "build", "--framework-inference", "true"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                framework_inference: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "framework_inference: flag set to true"
    )]
    #[test_case::test_case(
		&["turbo", "run", "build", "--framework-inference",
    "false"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                framework_inference: false,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "framework_inference: flag set to false"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                env_mode: EnvMode::Infer,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "env_mode: default infer"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--env-mode"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                env_mode: EnvMode::Infer,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "env_mode: not fully-specified"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--env-mode", "infer"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                env_mode: EnvMode::Infer,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "env_mode: specified infer"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--env-mode", "loose"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                env_mode: EnvMode::Loose,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "env_mode: specified loose"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--env-mode", "strict"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                env_mode: EnvMode::Strict,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "env_mode: specified strict"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "lint", "test"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string(), "lint".to_string(), "test".to_string()],
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--cache-dir", "foobar"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                cache_dir: Some(Utf8PathBuf::from("foobar")),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--cache-workers", "100"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                cache_workers: 100,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--concurrency", "20"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                concurrency: Some("20".to_string()),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--continue"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                continue_execution: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--dry-run"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                dry_run: Some(DryRunMode::Text),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--dry-run", "json"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                dry_run: Some(DryRunMode::Json),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--filter", "water", "--filter", "earth", "--filter", "fire", "--filter", "air"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                filter: vec![
                    "water".to_string(),
                    "earth".to_string(),
                    "fire".to_string(),
                    "air".to_string()
                ],
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "-F", "water", "-F", "earth", "-F", "fire", "-F", "air"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                filter: vec![
                    "water".to_string(),
                    "earth".to_string(),
                    "fire".to_string(),
                    "air".to_string()
                ],
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--filter", "water", "-F", "earth", "--filter", "fire", "-F", "air"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                filter: vec![
                    "water".to_string(),
                    "earth".to_string(),
                    "fire".to_string(),
                    "air".to_string()
                ],
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--force"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                force: Some(Some(true)),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--global-deps", ".env"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                global_deps: vec![".env".to_string()],
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&[ "turbo", "run", "build", "--global-deps", ".env", "--global-deps", ".env.development"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                global_deps: vec![".env".to_string(), ".env.development".to_string()],
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--graph"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                graph: Some("".to_string()),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--graph", "out.html"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                graph: Some("out.html".to_string()),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--filter", "[main]", "--ignore", "foo.js"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                ignore: vec!["foo.js".to_string()],
                filter: vec![String::from("[main]")],
                ..get_default_run_args()
            }))),
            ..Args::default()
        } ;
        "single ignore"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--filter", "[main]", "--ignore", "foo.js", "--ignore", "bar.js"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                ignore: vec!["foo.js".to_string(), "bar.js".to_string()],
                filter: vec![String::from("[main]")],
                ..get_default_run_args()
            }))),
            ..Args::default()
        } ;
        "multiple ignores"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--scope", "test", "--include-dependencies"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                include_dependencies: true,
                scope: vec!["test".to_string()],
                ..get_default_run_args()
            }))),
            ..Args::default()
        } ;
        "include dependencies"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--no-cache"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                no_cache: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--no-daemon"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                no_daemon: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--daemon"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                daemon: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--scope", "test", "--no-deps"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                scope: vec!["test".to_string()],
                no_deps: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
        } ;
        "no deps"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--output-logs", "full"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                output_logs: Some(OutputLogsMode::Full),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--output-logs", "none"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                output_logs: Some(OutputLogsMode::None),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--output-logs", "hash-only"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                output_logs: Some(OutputLogsMode::HashOnly),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--log-order", "stream"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                log_order: LogOrder::Stream,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--log-order", "grouped"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                log_order: LogOrder::Grouped,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--log-prefix", "auto"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                log_prefix: LogPrefix::Auto,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--log-prefix", "none"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                log_prefix: LogPrefix::None,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--log-prefix", "task"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                log_prefix: LogPrefix::Task,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                log_order: LogOrder::Auto,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--parallel"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                parallel: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--profile", "profile_out"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                profile: Some("profile_out".to_string()),
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    // remote-only flag tests
    #[test_case::test_case(
		&["turbo", "run", "build"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                remote_only: false,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "remote_only default to false"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--remote-only"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                remote_only: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "remote_only with no value, means true"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--remote-only", "true"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                remote_only: true,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "remote_only=true works"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--remote-only", "false"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                remote_only: false,
                ..get_default_run_args()
            }))),
            ..Args::default()
		} ;
        "remote_only=false works"
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--scope", "foo", "--scope", "bar"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                scope: vec!["foo".to_string(), "bar".to_string()],
                ..get_default_run_args()
            }))),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "run", "build", "--scope", "test", "--since", "foo"],
        Args {
            command: Some(Command::Run(Box::new(RunArgs {
                tasks: vec!["build".to_string()],
                scope: vec!["test".to_string()],
                since: Some("foo".to_string()),
                ..get_default_run_args()
            }))),
            ..Args::default()
        } ;
        "scope and since"
	)]
    #[test_case::test_case(
		&["turbo", "build"],
        Args {
            run_args: Some(RunArgs {
                tasks: vec!["build".to_string()],
                ..get_default_run_args()
            }),
            ..Args::default()
        }
	)]
    #[test_case::test_case(
		&["turbo", "build", "lint", "test"],
        Args {
            run_args: Some(RunArgs {
                tasks: vec!["build".to_string(), "lint".to_string(), "test".to_string()],
                ..get_default_run_args()
            }),
            ..Args::default()
        }
	)]
    fn test_parse_run(args: &[&str], expected: Args) {
        assert_eq!(Args::try_parse_from(args).unwrap(), expected);
    }

    fn test_serde() {
        // Test that ouput-logs is not serialized by default
        assert_eq!(
            serde_json::to_string(&Args::try_parse_from(["turbo", "run", "build"]).unwrap())
                .unwrap()
                .contains("\"output_logs\":null"),
            true
        );
    }
    #[test_case::test_case(
        &["turbo", "run", "build", "--daemon", "--no-daemon"],
        "cannot be used with '--no-daemon'" ;
        "daemon and no-daemon at the same time"
    )]
    #[test_case::test_case(
        &["turbo", "run", "build", "--ignore", "foo/**"],
        "the following required arguments were not provided" ;
        "ignore without filter or scope"
    )]
    #[test_case::test_case(
        &["turbo", "run", "build", "--since", "foo"],
        "the following required arguments were not provided" ;
        "since without filter or scope"
    )]
    #[test_case::test_case(
        &["turbo", "run", "build", "--include-dependencies"],
        "the following required arguments were not provided" ;
        "include-dependencies without filter or scope"
    )]
    #[test_case::test_case(
        &["turbo", "run", "build", "--no-deps"],
        "the following required arguments were not provided" ;
        "no-deps without filter or scope"
    )]
    fn test_parse_run_failures(args: &[&str], expected: &str) {
        assert_matches!(
            Args::try_parse_from(args),
            Err(err) if err.to_string().contains(expected)
        );
    }

    #[test]
    fn test_parse_bin() {
        assert_eq!(
            Args::try_parse_from(["turbo", "bin"]).unwrap(),
            Args {
                command: Some(Command::Bin {}),
                ..Args::default()
            }
        );

        CommandTestCase {
            command: "bin",
            command_args: vec![],
            global_args: vec![vec!["--cwd", "../examples/with-yarn"]],
            expected_output: Args {
                command: Some(Command::Bin {}),
                cwd: Some(Utf8PathBuf::from("../examples/with-yarn")),
                ..Args::default()
            },
        }
        .test();
    }

    #[test]
    fn test_parse_login() {
        assert_eq!(
            Args::try_parse_from(["turbo", "login"]).unwrap(),
            Args {
                command: Some(Command::Login { sso_team: None }),
                ..Args::default()
            }
        );

        CommandTestCase {
            command: "login",
            command_args: vec![],
            global_args: vec![vec!["--cwd", "../examples/with-yarn"]],
            expected_output: Args {
                command: Some(Command::Login { sso_team: None }),
                cwd: Some(Utf8PathBuf::from("../examples/with-yarn")),
                ..Args::default()
            },
        }
        .test();

        CommandTestCase {
            command: "login",
            command_args: vec![vec!["--sso-team", "my-team"]],
            global_args: vec![vec!["--cwd", "../examples/with-yarn"]],
            expected_output: Args {
                command: Some(Command::Login {
                    sso_team: Some("my-team".to_string()),
                }),
                cwd: Some(Utf8PathBuf::from("../examples/with-yarn")),
                ..Args::default()
            },
        }
        .test();
    }

    #[test]
    fn test_parse_logout() {
        assert_eq!(
            Args::try_parse_from(["turbo", "logout"]).unwrap(),
            Args {
                command: Some(Command::Logout {}),
                ..Args::default()
            }
        );

        CommandTestCase {
            command: "logout",
            command_args: vec![],
            global_args: vec![vec!["--cwd", "../examples/with-yarn"]],
            expected_output: Args {
                command: Some(Command::Logout {}),
                cwd: Some(Utf8PathBuf::from("../examples/with-yarn")),
                ..Args::default()
            },
        }
        .test();
    }

    #[test]
    fn test_parse_unlink() {
        assert_eq!(
            Args::try_parse_from(["turbo", "unlink"]).unwrap(),
            Args {
                command: Some(Command::Unlink {
                    target: crate::cli::LinkTarget::RemoteCache
                }),
                ..Args::default()
            }
        );

        CommandTestCase {
            command: "unlink",
            command_args: vec![],
            global_args: vec![vec!["--cwd", "../examples/with-yarn"]],
            expected_output: Args {
                command: Some(Command::Unlink {
                    target: crate::cli::LinkTarget::RemoteCache,
                }),
                cwd: Some(Utf8PathBuf::from("../examples/with-yarn")),
                ..Args::default()
            },
        }
        .test();
    }

    #[test]
    fn test_parse_prune() {
        let default_prune = Command::Prune {
            scope: None,
            scope_arg: Some(vec!["foo".into()]),
            docker: false,
            output_dir: "out".to_string(),
        };

        assert_eq!(
            Args::try_parse_from(["turbo", "prune", "foo"]).unwrap(),
            Args {
                command: Some(default_prune.clone()),
                ..Args::default()
            }
        );

        CommandTestCase {
            command: "prune",
            command_args: vec![vec!["foo"]],
            global_args: vec![vec!["--cwd", "../examples/with-yarn"]],
            expected_output: Args {
                command: Some(default_prune),
                cwd: Some(Utf8PathBuf::from("../examples/with-yarn")),
                ..Args::default()
            },
        }
        .test();

        assert_eq!(
            Args::try_parse_from(["turbo", "prune", "--scope", "bar"]).unwrap(),
            Args {
                command: Some(Command::Prune {
                    scope: Some(vec!["bar".to_string()]),
                    scope_arg: None,
                    docker: false,
                    output_dir: "out".to_string(),
                }),
                ..Args::default()
            }
        );

        assert_eq!(
            Args::try_parse_from(["turbo", "prune", "foo", "bar"]).unwrap(),
            Args {
                command: Some(Command::Prune {
                    scope: None,
                    scope_arg: Some(vec!["foo".to_string(), "bar".to_string()]),
                    docker: false,
                    output_dir: "out".to_string(),
                }),
                ..Args::default()
            }
        );

        assert_eq!(
            Args::try_parse_from(["turbo", "prune", "--docker", "foo"]).unwrap(),
            Args {
                command: Some(Command::Prune {
                    scope: None,
                    scope_arg: Some(vec!["foo".into()]),
                    docker: true,
                    output_dir: "out".to_string(),
                }),
                ..Args::default()
            }
        );

        assert_eq!(
            Args::try_parse_from(["turbo", "prune", "--out-dir", "dist", "foo"]).unwrap(),
            Args {
                command: Some(Command::Prune {
                    scope: None,
                    scope_arg: Some(vec!["foo".into()]),
                    docker: false,
                    output_dir: "dist".to_string(),
                }),
                ..Args::default()
            }
        );

        CommandTestCase {
            command: "prune",
            command_args: vec![vec!["foo"], vec!["--out-dir", "dist"], vec!["--docker"]],
            global_args: vec![],
            expected_output: Args {
                command: Some(Command::Prune {
                    scope: None,
                    scope_arg: Some(vec!["foo".into()]),
                    docker: true,
                    output_dir: "dist".to_string(),
                }),
                ..Args::default()
            },
        }
        .test();

        CommandTestCase {
            command: "prune",
            command_args: vec![vec!["foo"], vec!["--out-dir", "dist"], vec!["--docker"]],
            global_args: vec![vec!["--cwd", "../examples/with-yarn"]],
            expected_output: Args {
                command: Some(Command::Prune {
                    scope: None,
                    scope_arg: Some(vec!["foo".into()]),
                    docker: true,
                    output_dir: "dist".to_string(),
                }),
                cwd: Some(Utf8PathBuf::from("../examples/with-yarn")),
                ..Args::default()
            },
        }
        .test();

        CommandTestCase {
            command: "prune",
            command_args: vec![
                vec!["--out-dir", "dist"],
                vec!["--docker"],
                vec!["--scope", "foo"],
            ],
            global_args: vec![],
            expected_output: Args {
                command: Some(Command::Prune {
                    scope: Some(vec!["foo".to_string()]),
                    scope_arg: None,
                    docker: true,
                    output_dir: "dist".to_string(),
                }),
                ..Args::default()
            },
        }
        .test();
    }

    #[test]
    fn test_pass_through_args() {
        assert_eq!(
            Args::try_parse_from(["turbo", "run", "build", "--", "--script-arg=42"]).unwrap(),
            Args {
                command: Some(Command::Run(Box::new(RunArgs {
                    tasks: vec!["build".to_string()],
                    pass_through_args: vec!["--script-arg=42".to_string()],
                    ..get_default_run_args()
                }))),
                ..Args::default()
            }
        );

        assert_eq!(
            Args::try_parse_from([
                "turbo",
                "run",
                "build",
                "--",
                "--script-arg=42",
                "--foo",
                "--bar",
                "bat"
            ])
            .unwrap(),
            Args {
                command: Some(Command::Run(Box::new(RunArgs {
                    tasks: vec!["build".to_string()],
                    pass_through_args: vec![
                        "--script-arg=42".to_string(),
                        "--foo".to_string(),
                        "--bar".to_string(),
                        "bat".to_string()
                    ],
                    ..get_default_run_args()
                }))),
                ..Args::default()
            }
        );
    }

    #[test]
    fn test_parse_prune_no_mixed_arg_and_flag() {
        assert!(Args::try_parse_from(["turbo", "prune", "foo", "--scope", "bar"]).is_err(),);
    }

    #[test]
    fn test_verbosity_serialization() -> Result<(), serde_json::Error> {
        assert_eq!(
            serde_json::to_string(&Verbosity {
                verbosity: None,
                v: 0
            })?,
            "0"
        );
        assert_eq!(
            serde_json::to_string(&Verbosity {
                verbosity: Some(3),
                v: 0
            })?,
            "3"
        );
        assert_eq!(
            serde_json::to_string(&Verbosity {
                verbosity: None,
                v: 3
            })?,
            "3"
        );
        Ok(())
    }
    #[test]
    fn test_parse_gen() {
        let default_gen = Command::Generate {
            tag: "latest".to_string(),
            generator_name: None,
            config: None,
            root: None,
            args: vec![],
            command: None,
        };

        assert_eq!(
            Args::try_parse_from(["turbo", "gen"]).unwrap(),
            Args {
                command: Some(default_gen.clone()),
                ..Args::default()
            }
        );

        assert_eq!(
            Args::try_parse_from([
                "turbo",
                "gen",
                "--args",
                "my long arg string",
                "my-second-arg"
            ])
            .unwrap(),
            Args {
                command: Some(Command::Generate {
                    tag: "latest".to_string(),
                    generator_name: None,
                    config: None,
                    root: None,
                    args: vec![
                        "my long arg string".to_string(),
                        "my-second-arg".to_string()
                    ],
                    command: None,
                }),
                ..Args::default()
            }
        );

        assert_eq!(
            Args::try_parse_from([
                "turbo",
                "gen",
                "--tag",
                "canary",
                "--config",
                "~/custom-gen-config/gen",
                "my-generator"
            ])
            .unwrap(),
            Args {
                command: Some(Command::Generate {
                    tag: "canary".to_string(),
                    generator_name: Some("my-generator".to_string()),
                    config: Some("~/custom-gen-config/gen".to_string()),
                    root: None,
                    args: vec![],
                    command: None,
                }),
                ..Args::default()
            }
        );
    }

    #[test]
    fn test_go_fallback_conflicts_with_remote_read_only() {
        assert!(Args::try_parse_from([
            "turbo",
            "build",
            "--remote-cache-read-only",
            "--go-fallback",
        ])
        .unwrap_err()
        .to_string()
        .contains(
            "the argument '--remote-cache-read-only [<BOOL>]' cannot be used with '--go-fallback"
        ));
        assert!(Args::try_parse_from([
            "turbo",
            "--go-fallback",
            "--remote-cache-read-only",
            "true",
            "build",
        ])
        .unwrap_err()
        .to_string()
        .contains(
            "the argument '--go-fallback' cannot be used with '--remote-cache-read-only [<BOOL>]'"
        ));
        assert!(Args::try_parse_from([
            "turbo",
            "run",
            "build",
            "--remote-cache-read-only",
            "--go-fallback",
        ])
        .unwrap_err()
        .to_string()
        .contains(
            "the argument '--remote-cache-read-only [<BOOL>]' cannot be used with '--go-fallback"
        ));
        assert!(Args::try_parse_from(["turbo", "build", "--go-fallback"]).is_ok(),);
        assert!(Args::try_parse_from(["turbo", "build", "--remote-cache-read-only",]).is_ok(),);
    }

    #[test]
    fn test_profile_usage() {
        assert!(Args::try_parse_from(["turbo", "build", "--profile", ""]).is_err());
        assert!(Args::try_parse_from(["turbo", "build", "--anon-profile", ""]).is_err());
        assert!(Args::try_parse_from(["turbo", "build", "--profile", "foo.json"]).is_ok());
        assert!(Args::try_parse_from(["turbo", "build", "--anon-profile", "foo.json"]).is_ok());
        assert!(Args::try_parse_from([
            "turbo",
            "build",
            "--profile",
            "foo.json",
            "--anon-profile",
            "bar.json"
        ])
        .is_err());
    }
}
