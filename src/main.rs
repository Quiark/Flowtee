mod tmux;
mod cfg;

use clap::{Parser, Subcommand};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::de::DeserializeOwned;
use std::io::Read;
use std::process::Command;

#[derive(Parser)]
#[command(name = "flowtee")]
#[command(about = "A command execution and output capture tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// testing purposes, Execute a command and capture its output
    Exec {
        /// The command to execute
        #[arg(required = true)]
        command: String,

        /// Arguments to pass to the command
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,

        /// Search for these strings in the output (ANSI codes will be stripped for matching)
        #[arg(short = 's', long = "search")]
        search: Vec<String>,
    },
    Step {
        /// The name of the workflow step to execute
        #[arg(short = 's', required = true)]
        name: String,

        /// The workflow to use (config file)
        #[arg(short = 'w', default_value = "docx")]
        workflow: String,

        /// Executes the step but skips the remoting to tmux
        #[arg(short = 'h', default_value = "0")]
        here: bool,

    }
}

fn main() {
    //println!("{}", dirs::home_dir().unwrap().display());
    // use standard ~/.config/flowtee/config.yaml path
    let cli = Cli::parse();

    let r = match cli.command {
        Commands::Exec {
            command,
            args,
            search,
        } => {
            execute_command_with_pty(&command, &args, &search)
        },
        cmd @Commands::Step { .. } => run_step(&cmd)
    };

    match r {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn load_config<T: DeserializeOwned>(name: &str) -> anyhow::Result<T> {
    let cfg_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".config")
        .join("flowtee")
        .join(name);
    let workflow: T = serde_yaml::from_reader(std::fs::File::open(cfg_path)? /*("Failed to open config file")*/)?;
        //.expect("Failed to parse config file");
    Ok(workflow)
}

fn run_step(c: &Commands) -> anyhow::Result<()> {
    let Commands::Step { name, workflow, here } = c else { panic!("was matched against step but isnt") };
    let w: cfg::Workflow = load_config::<cfg::Workflow>(&workflow)?;
    let step = w.steps.iter().find(|s| s.name == *name)
        .ok_or_else(|| anyhow::anyhow!("Step '{}' not found in workflow", name))?;

    Ok(())
}

fn execute_command_with_pty(command: &str, args: &[String], search: &[String]) -> anyhow::Result<()> {
    let pty_system = native_pty_system();

    // Detect terminal size, fall back to defaults if detection fails
    let (cols, rows) = terminal_size::terminal_size()
        .map(|(terminal_size::Width(w), terminal_size::Height(h))| (w, h))
        .unwrap_or((80, 24));

    let size = PtySize {
        rows: rows as u16,
        cols: cols as u16,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = match pty_system.openpty(size) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("Failed to open PTY: {}", e);
            std::process::exit(1);
        }
    };

    let mut cmd = CommandBuilder::new(command);
    cmd.args(args);

    // Preserve current working directory
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Failed to spawn command: {}", e);
            std::process::exit(1);
        }
    };

    // Drop the slave to avoid deadlock
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut buffer = [0u8; 1024];
    let mut captured_output = Vec::new();

    // Stream output in real-time and capture for searching
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(n) => {
                //println!(">> {} bytes", n);
                // Capture output for searching
                if !search.is_empty() {
                    let stripped = strip_ansi_escapes::strip(&buffer[..n]);
                    captured_output.extend_from_slice(&stripped);
                    search_output(&captured_output, search);
                }

                // Print immediately as data arrives
                print!("{}", String::from_utf8_lossy(&buffer[..n]));
                use std::io::Write;
                std::io::stdout().flush().unwrap();
            }
            Err(e) => {
                // Break on error (including when child exits)
                if e.kind() != std::io::ErrorKind::Interrupted {
                    break;
                }
            }
        }
    }

    // Wait for the child to complete
    let status = match child.wait() {
        Ok(status) => status,
        Err(e) => {
            eprintln!("Failed to wait for child: {}", e);
            std::process::exit(1);
        }
    };
    Ok(())
}

fn search_output(output: &[u8], search_terms: &[String]) {
    let stripped_str = String::from_utf8_lossy(&output);

    println!("\n{}", "=".repeat(60));
    println!("Search results:");
    println!("{}", "=".repeat(60));

    let mut found_any = false;
    for term in search_terms {
        let pstart = (stripped_str.len() as i64 - (term.len() as i64 * 2)).max(0) as usize;
        let count = stripped_str[pstart..].matches(term.as_str()).count();
        if count > 0 {
            println!("  '{}': found {} time(s)", term, count);
            found_any = true;
        } else {
            println!("  '{}': not found", term);
        }
    }

    if !found_any {
        println!("  No search terms were found in the output");
    }
}
