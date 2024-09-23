use std::{
    io,
    process::{Command, Stdio},
};

use thiserror::Error;
use tracing::debug;
use turborepo_telemetry::events::command::CommandEventBuilder;
use which::which;

use crate::{
    child::spawn_child,
    cli::{GenerateCommand, GeneratorCustomArgs},
};

const LATEST_TAG: &str = "latest";

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unable to run generate - missing requirements (npx): {0}")]
    NpxNotFound(#[source] which::Error),
    #[error("Failed to run npx: {0}")]
    NpxFailed(#[source] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn cache_latest() -> Result<i32, Error> {
    let npx_path = which("npx").map_err(Error::NpxNotFound)?;
    let mut npx = Command::new(npx_path);
    npx.arg("--yes")
        .arg(format!("@turbo/gen@{}", LATEST_TAG))
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let child = spawn_child(npx).map_err(Error::NpxFailed)?;
    let exit_code = child.wait().map_err(Error::NpxFailed)?.code().unwrap_or(2);
    Ok(exit_code)
}

fn call_turbo_gen(command: &str, tag: &Option<String>, raw_args: &str) -> Result<i32, Error> {
    let version = tag.clone().map_or(String::new(), |t| format!("@{}", t));
    debug!(
        "Running @turbo/gen{} with command `{}` and args {:?}",
        version, command, raw_args
    );
    let npx_path = which("npx").map_err(Error::NpxNotFound)?;
    let mut npx = Command::new(npx_path);
    npx.arg("--yes")
        .arg(format!("@turbo/gen{}", version))
        .arg("raw")
        .arg(command)
        .args(["--json", raw_args])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let child = spawn_child(npx).map_err(Error::NpxFailed)?;
    let exit_code = child.wait().map_err(Error::NpxFailed)?.code().unwrap_or(2);
    Ok(exit_code)
}

pub fn run(
    tag: &Option<String>,
    command: &Option<Box<GenerateCommand>>,
    args: &GeneratorCustomArgs,
    telemetry: CommandEventBuilder,
) -> Result<(), Error> {
    telemetry.track_generator_tag(tag);
    // check if a subcommand was passed
    if let Some(box GenerateCommand::Workspace(workspace_args)) = command {
        let raw_args = serde_json::to_string(&workspace_args)?;
        telemetry.track_generator_option("workspace");
        call_turbo_gen("workspace", tag, &raw_args)?;
    } else {
        // if no subcommand was passed, run the generate command as default
        let raw_args = serde_json::to_string(&args)?;
        telemetry.track_generator_option("run");
        call_turbo_gen("run", tag, &raw_args)?;
    }

    // lazy refresh the latest version
    if tag.is_none() || *tag == Some(LATEST_TAG.to_string()) {
        match cache_latest() {
            Ok(0) => {
                debug!("Successfully cached latest version of @turbo/gen");
            }
            Ok(code) => {
                debug!(
                    "Failed to cache latest version of @turbo/gen with exit code {}",
                    code
                );
            }
            Err(e) => {
                debug!("Failed to cache latest version of @turbo/gen: {}", e);
            }
        }
    }

    Ok(())
}
