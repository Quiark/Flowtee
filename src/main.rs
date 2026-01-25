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
    /// Execute a predefined workflow step
    Step {
        /// The name of the workflow step to execute
        #[arg(short = 's', required = true)]
        name: String,

        /// The workflow to use (config file)
        #[arg(short = 'w', default_value = "docx")]
        workflow: String,

        /// Executes the step but skips the remoting to tmux
        #[arg(short = 'l', default_value = "false")]
        local: bool,

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
            // execute_command_with_pty(&command, &args, &search, )
            Ok(())
        },
        cmd @Commands::Step { .. } => cli_run_step(&cmd)
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

fn cli_run_step(c: &Commands) -> anyhow::Result<()> {
    let Commands::Step { name, workflow, local } = c else { panic!("was matched against step but isnt") };
    let w: cfg::Workflow = load_config::<cfg::Workflow>(&format!("{}.yaml", workflow))?;
    let app = cfg::AppObj {
        workflow: w,
        workflow_name: workflow.clone(),
    };
    run_step_by_name(name, *local, &app)
}

fn run_step_by_name(name: &str, local: bool, app: &cfg::AppObj) -> anyhow::Result<()> {
    let step = app.workflow.steps.iter().find(|s| s.name == *name)
        .ok_or_else(|| anyhow::anyhow!("Step '{}' not found in workflow", name))?;

    // Check if we should remote to tmux
    if let Some(tmux_opt) = &step.tmux {
        if !local {
            // Remote execution via tmux
            let target = format!("{}:{}", tmux_opt.sess, tmux_opt.win);

            tmux::careful_run_command(&target, &format!("ft step -l -w {} -s {name}", app.workflow_name), tmux_opt.fish_vi_mode)?;
            //tmux::careful_run_command(&target, &step.command, tmux_opt.fish_vi_mode)?;

            return Ok(());
        }
    }

    // Local execution through shell
    // Run the command through /bin/sh -c to handle shell syntax
    execute_command_with_pty("/bin/sh", &["-c".to_string(), step.command.clone()], &step,  &app)
}

fn execute_command_with_pty(command: &str, args: &[String], step: &cfg::WorkflowStep, app: &cfg::AppObj) -> anyhow::Result<()> {
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
    let mut state: Option<cfg::Impulse> = None;

    // Stream output in real-time and capture for searching
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(n) => {
                //println!(">> {} bytes", n);
                // Capture output for searching
                let stripped = strip_ansi_escapes::strip(&buffer[..n]);
                captured_output.extend_from_slice(&stripped);


                // only search and take action if we haven't already
                if state.is_none() {
                    if let Some(i) = search_output(&captured_output, step, app) {
                        state = Some(i)
                    }
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

fn search_output(output: &[u8], step: &cfg::WorkflowStep, app: &cfg::AppObj) -> Option<cfg::Impulse> {
    if let Some(scan_err) = &step.scan_err {
        if search_output_one(output, scan_err) {
            return Some(cfg::Impulse::Error);
        }
    }
    if search_output_one(output, &step.scan_ok) {
        return Some(cfg::Impulse::Success);
    }
    return None;
}

fn search_output_one(output: &[u8], term: &str) -> bool {
    let pstart = (output.len() as i64 - (term.len() as i64 * 2)).max(0) as usize;
    let stripped_str = String::from_utf8_lossy(&output[pstart..]);
    let count = stripped_str.matches(term).count();
    return count > 0;
}

fn take_action(from: &cfg::WorkflowStep, impulse: &cfg::Impulse, app: &cfg::AppObj) -> anyhow::Result<()> {
    match impulse {
        cfg::Impulse::Success => {
            from.links.as_ref()
                .and_then(|l| l.on_ok.as_ref())
                .and_then(|s| Some(run_step_by_name(s, false, app)))
                .unwrap_or(Ok(()))
        }
        cfg::Impulse::Error => {
            from.links.as_ref()
                .and_then(|l| l.on_err.as_ref())
                .and_then(|s| Some(run_step_by_name(s, false, app)))
                .unwrap_or(Ok(()))
        }
    }
}
