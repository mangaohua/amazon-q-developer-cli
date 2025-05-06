use std::process::ExitCode;

use anstream::eprintln;
use clap::{
    Subcommand,
    ValueEnum,
};
use eyre::Result;

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum Build {
    Production,
    #[value(alias = "staging")]
    Beta,
    #[value(hide = true, alias = "dev")]
    Develop,
}

impl std::fmt::Display for Build {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Build::Production => f.write_str("production"),
            Build::Beta => f.write_str("beta"),
            Build::Develop => f.write_str("develop"),
        }
    }
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum App {
    Dashboard,
    Autocomplete,
}

impl std::fmt::Display for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            App::Dashboard => f.write_str("dashboard"),
            App::Autocomplete => f.write_str("autocomplete"),
        }
    }
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum AutocompleteWindowDebug {
    On,
    Off,
}

#[derive(Debug, ValueEnum, Clone, PartialEq, Eq)]
pub enum AccessibilityAction {
    Refresh,
    Reset,
    Prompt,
    Open,
    Status,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq, ValueEnum)]
pub enum TISAction {
    Enable,
    Disable,
    Select,
    Deselect,
}

#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
#[derive(Debug, Subcommand, Clone, PartialEq, Eq)]
pub enum InputMethodDebugAction {
    Install {
        bundle_path: Option<PathBuf>,
    },
    Uninstall {
        bundle_path: Option<PathBuf>,
    },
    List,
    Status {
        bundle_path: Option<PathBuf>,
    },
    Source {
        bundle_identifier: String,
        #[arg(value_enum)]
        action: TISAction,
    },
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum DebugSubcommand {
    RefreshAuthToken,
}

impl DebugSubcommand {
    pub async fn execute(&self) -> Result<ExitCode> {
        match self {
            DebugSubcommand::RefreshAuthToken => match crate::fig_auth::refresh_token().await? {
                Some(_) => eprintln!("Refreshed token"),
                None => {
                    eprintln!("No token to refresh");
                    return Ok(ExitCode::FAILURE);
                },
            },
        }
        Ok(ExitCode::SUCCESS)
    }
}
