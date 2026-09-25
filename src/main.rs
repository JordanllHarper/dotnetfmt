mod cli;
mod editorconfig;
mod formatter;

use std::error::Error;
use std::fs;

use clap::Parser;
use cli::{Cli, Command};
use formatter::format;

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let cwd = std::env::current_dir()?;
    let config = editorconfig::read(&cwd)?;

    match cli.command {
        Command::File { path } => {
            let contents = fs::read_to_string(&path)?;
            let formatted = format(&contents, &config)?;
            print!("{}", formatted);
            Ok(())
        }
        Command::Workspace { .. } => Err("workspace formatting isn't implemented yet".into()),
    }
}
