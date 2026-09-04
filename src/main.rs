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
mod formatter;

use clap::Parser;
use cli::Cli;
use formatter::format;

fn main() {
    let cli = Cli::parse();
    println!("{:#?}", cli);

    let cwd = std::env::current_dir().expect("failed to read current directory");
    match editorconfig::read(&cwd) {
        Ok(config) => {
            println!("{:#?}", config);

            // Temporary smoke test for the formatter while it's under
            // construction -- will be replaced with real file I/O once
            // the CLI subcommands are wired up.
            let sample = concat!(
                "var                x =               5;\r\n",
                "var y=10;\r\n",
                "var z=someFunction();\r\n",
                "var w = someFunction (   )   ;\r\n",
                "var a=someFunction(1);\r\n",
                "var b = someFunction(   c   );\r\n",
                "var d=someFunction(e,f,g);\r\n",
                "var h=someFunction(e+f);",
            );
            match format(sample, &config) {
                Ok(formatted) => {
                    for (before, after) in sample.split("\r\n").zip(formatted.split("\r\n")) {
                        println!("before: {}", before);
                        println!("after:  {}\n", after);
                    }
                }
                Err(err) => match err {},
            }
        }
        Err(err) => eprintln!("error: {}", err),
    }
}
