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
                "public   static   int Add(int a,int b)\r\n",
                "{\r\n",
                "int sum=a+b;\r\n",
                "if (sum>0)\r\n",
                "{\r\n",
                "Console.WriteLine(\"positive: \"+sum);\r\n",
                "}\r\n",
                "List<int> seen=new List<int>();\r\n",
                "var ok=sum!=0&&sum<100;\r\n",
                "sum+=1;\r\n",
                "return sum;\r\n",
                "}",
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
