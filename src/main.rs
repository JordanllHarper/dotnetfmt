/*
 *  Formatting needs to take in a file and spit out a formatted file
 *
 *  - Start by taking in some file paths to get the file handles and read in the file
 *  - analyze the code
 *  - spit out either to stdin or to the input filepath
 *
 *
 *  What does analyze the code mean?
 *  - we need to look at the code structure and determine two concepts:
 *      - indent (col) matching - how far should the code be indented? How many spaces or tabs are
 *      needed to fulfill that indentation?
 *      - spacing matching - spacing between variables, function calls, etc.
 *      - character matching - using tabs or spaces, how many cols is a tab
 *
 *
*/
mod cli;
mod editorconfig;

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();
    println!("{:#?}", cli);

    let cwd = std::env::current_dir().expect("failed to read current directory");
    match editorconfig::read(&cwd) {
        Ok(config) => println!("{:#?}", config),
        Err(err) => eprintln!("error: {}", err),
    }
}
