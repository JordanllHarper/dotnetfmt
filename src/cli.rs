//! Command-line interface definition for dotnet_fmt.
//!
//! Two invocation shapes are supported:
//!
//!   dotnet_fmt file <path>
//!       Format a single C# file.
//!
//!   dotnet_fmt workspace [--workspace <path>]
//!       Format every C# file under a workspace directory. When
//!       `--workspace` is omitted, the current directory is used.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "dotnet_fmt",
    about = "A whitespace formatter for C#, resembling `dotnet format whitespace`",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Format a single C# file
    File {
        /// Path to the C# file to format
        path: PathBuf,
    },
    /// Format all C# files in a workspace
    Workspace {
        /// Path to the workspace root. Defaults to the current directory.
        #[arg(long, short)]
        workspace: Option<PathBuf>,
    },
}
