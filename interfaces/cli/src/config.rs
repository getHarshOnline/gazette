//! Explicit changes to the user's Gazette preferences.

use gazette_sdk::configuration;
use std::{env, path::PathBuf, process::ExitCode};

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(subcommand)]
    operation: Operation,
}

#[derive(clap::Subcommand)]
enum Operation {
    /// Choose a default used when no request or local folder selects a workspace.
    Set {
        #[command(subcommand)]
        setting: Setting,
    },
}

#[derive(clap::Subcommand)]
enum Setting {
    /// Save a workspace folder or configuration file as this user's default.
    Workspace { path: PathBuf },
}

pub(super) fn run(args: Args) -> ExitCode {
    let result = (|| -> Result<_, Box<dyn std::error::Error>> {
        let Operation::Set {
            setting: Setting::Workspace { path },
        } = args.operation;
        let start = env::current_dir()?;
        let location = configuration::locate_workspace(configuration::WorkspaceLookup {
            request: Some(&path),
            client: None,
            start: &start,
            default: None,
        })?
        .ok_or("no workspace selected")?;
        let file = configuration::user_configuration_path()?;
        configuration::set_default_workspace(
            &file,
            &location.root.join(location.workspace.as_str()),
        )?;
        Ok(serde_json::json!({"configuration":file,"workspace":location}))
    })();
    match result {
        Ok(value) => super::write_json(&value),
        Err(error) => {
            eprintln!("gazette: {error}");
            ExitCode::FAILURE
        }
    }
}
