mod cfg;
mod tmux;

use clap::{Parser, Subcommand};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::de::DeserializeOwned;
use std::io::Read;
use std::sync::{Mutex, OnceLock};

static RUNNING_PGIDS: OnceLock<Mutex<Vec<i32>>> = OnceLock::new();

fn running_pgids() -> &'static Mutex<Vec<i32>> {
    RUNNING_PGIDS.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(unix)]
fn kill_process_group(pgid: i32) {
    unsafe {
        libc::kill(-pgid, libc::SIGTERM);
    }
}

fn terminate_all(exit_code: i32) -> ! {
    let pgids = running_pgids().lock().unwrap().clone();
    for pgid in pgids {
        #[cfg(unix)]
        {
            kill_process_group(pgid);
        }
    }

    std::process::exit(exit_code)
}

enum LinkAction<'a> {
    Step(&'a str),
    End,
}

fn resolve_link_action(link: &cfg::WorkflowLink) -> LinkAction<'_> {
    match link {
        cfg::WorkflowLink::StepName(s) => LinkAction::Step(s.as_str()),
        cfg::WorkflowLink::Typed(cfg::WorkflowLinkTyped::Step { step }) => {
            LinkAction::Step(step.as_str())
        }
        cfg::WorkflowLink::Typed(cfg::WorkflowLinkTyped::End) => LinkAction::End,
    }
}

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

        /// Path to a custom YAML file (overrides workflow name)
        #[arg(short = 'f', long = "file")]
        file: Option<String>,
    },
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
        }
        cmd @ Commands::Step { .. } => cli_run_step(&cmd),
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
    let workflow: T = serde_yaml::from_reader(
        std::fs::File::open(cfg_path)?, /*("Failed to open config file")*/
    )?;
    //.expect("Failed to parse config file");
    Ok(workflow)
}

fn load_config_from_path<T: DeserializeOwned>(path: &str) -> anyhow::Result<T> {
    let workflow: T = serde_yaml::from_reader(
        std::fs::File::open(path)?,
    )?;
    Ok(workflow)
}

fn cli_run_step(c: &Commands) -> anyhow::Result<()> {
    let Commands::Step {
        name,
        workflow,
        local,
        file,
    } = c
    else {
        panic!("was matched against step but isnt")
    };
    
    let w: cfg::Workflow = if let Some(file_path) = file {
        load_config_from_path::<cfg::Workflow>(file_path)?
    } else {
        load_config::<cfg::Workflow>(&format!("{}.yaml", workflow))?
    };
    
    let app = cfg::AppObj {
        workflow: w,
        workflow_name: workflow.clone(),
    };
    run_step_by_name(name, *local, &app)
}

fn run_step_by_name(name: &str, local: bool, app: &cfg::AppObj) -> anyhow::Result<()> {
    let step = app
        .workflow
        .steps
        .iter()
        .find(|s| s.name == *name)
        .ok_or_else(|| anyhow::anyhow!("Step '{}' not found in workflow", name))?;

    // Check if we should remote to tmux
    if let Some(tmux_opt) = &step.tmux {
        if !local {
            // Remote execution via tmux
            let target = format!("{}:{}", tmux_opt.sess, tmux_opt.win);

            tmux::careful_run_command(
                &target,
                &format!("ft step -l -w {} -s {name}", app.workflow_name),
                tmux_opt.fish_vi_mode,
            )?;
            //tmux::careful_run_command(&target, &step.command, tmux_opt.fish_vi_mode)?;

            return Ok(());
        }
    }

    // Local execution through shell
    // Run the command through /bin/sh -c to handle shell syntax
    execute_command_with_pty(
        "/bin/sh",
        &["-c".to_string(), step.command.clone()],
        &step,
        &app,
    )
}

fn execute_command_with_pty(
    command: &str,
    args: &[String],
    step: &cfg::WorkflowStep,
    app: &cfg::AppObj,
) -> anyhow::Result<()> {
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

    // Preserve current working directory (or override per-step)
    if let Some(pwd) = &step.pwd {
        cmd.cwd(pwd);
    } else if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }

    // Apply environment variables from step config
    if let Some(env_vars) = &step.env {
        for (key, value) in env_vars {
            cmd.env(key, value);
        }
    }

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Failed to spawn command: {}", e);
            std::process::exit(1);
        }
    };

    // Track process group so `type: end` can terminate everything.
    #[cfg(unix)]
    if let Some(pgid) = pair.master.process_group_leader() {
        running_pgids().lock().unwrap().push(pgid);
    }

    // Drop the slave to avoid deadlock
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut buffer = [0u8; 1024];
    let mut captured_output = Vec::new();
    let mut scan_state: Option<cfg::Impulse> = None;
    let mut scan_action_taken = false;

    let mut output_file = if let Some(outputs) = &step.outputs {
        Some(std::fs::File::create(&outputs.file)?)
    } else {
        None
    };

    // Stream output in real-time and capture for searching
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break, // EOF
            Ok(n) => {
                //println!(">> {} bytes", n);
                // Capture output for searching
                let stripped = strip_ansi_escapes::strip(&buffer[..n]);
                captured_output.extend_from_slice(&stripped);

                use std::io::Write;
                if let Some(ref mut file) = output_file {
                    file.write_all(&stripped)?;
                    file.flush()?;
                }

                // Scan match is independent from exit status.
                if scan_state.is_none() {
                    if let Some(event) = search_output(&captured_output, step) {
                        scan_state = Some(event);
                    }
                }

                if !scan_action_taken {
                    if let Some(event) = scan_state {
                        take_action_async(step, &event, app)?;
                        scan_action_taken = true;
                    }
                }

                // Print immediately as data arrives
                print!("{}", add_to_each_line(&format!("{}| ", step.name), &String::from_utf8_lossy(&buffer[..n])));
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

    let exit_event = if status.success() {
        cfg::Impulse::ExitOk
    } else {
        cfg::Impulse::ExitErr
    };
    take_action(step, &exit_event, app)?;

    Ok(())
}

fn add_to_each_line(prefix: &str, text: &str) -> String {
    text.replace("\n", &format!("\n{}", prefix))
    //text.lines()
    //    .map(|line| format!("{}{}", prefix, line))
    //    .collect::<Vec<String>>()
    //    .join("\n")
}

fn search_output(output: &[u8], step: &cfg::WorkflowStep) -> Option<cfg::Impulse> {
    if let Some(scan_err) = &step.scan_err {
        if search_output_one(output, scan_err) {
            return Some(cfg::Impulse::ScanErr);
        }
    }

    if let Some(scan_ok) = &step.scan_ok {
        if search_output_one(output, scan_ok) {
            return Some(cfg::Impulse::ScanOk);
        }
    }

    None
}

fn search_output_one(output: &[u8], term: &str) -> bool {
    let pstart = (output.len() as i64 - (term.len() as i64 * 2)).max(0) as usize;
    let stripped_str = String::from_utf8_lossy(&output[pstart..]);
    let count = stripped_str.matches(term).count();
    return count > 0;
}

fn take_action_async(
    from: &cfg::WorkflowStep,
    impulse: &cfg::Impulse,
    app: &cfg::AppObj,
) -> anyhow::Result<()> {
    let links = from.links.as_ref();

    let link = match impulse {
        cfg::Impulse::ScanOk => links
            .and_then(|l| l.on_scan_ok.as_ref())
            .or_else(|| links.and_then(|l| l.on_ok.as_ref())),
        cfg::Impulse::ScanErr => links
            .and_then(|l| l.on_scan_err.as_ref())
            .or_else(|| links.and_then(|l| l.on_err.as_ref())),
        cfg::Impulse::ExitOk => links
            .and_then(|l| l.on_exit_ok.as_ref())
            .or_else(|| links.and_then(|l| l.on_ok.as_ref())),
        cfg::Impulse::ExitErr => links
            .and_then(|l| l.on_exit_err.as_ref())
            .or_else(|| links.and_then(|l| l.on_err.as_ref())),
    };

    let Some(link) = link else {
        return Ok(());
    };

    if from.final_step.unwrap_or(false) {
        return Ok(());
    }

    match resolve_link_action(link) {
        LinkAction::Step(step_name) => {
            let step_name = step_name.to_string();
            let app_clone = app.clone();
            std::thread::spawn(move || {
                if let Err(e) = run_step_by_name(&step_name, false, &app_clone) {
                    eprintln!("Error running step {}: {}", step_name, e);
                }
            });
            Ok(())
        }
        LinkAction::End => {
            let code = match impulse {
                cfg::Impulse::ExitErr | cfg::Impulse::ScanErr => 1,
                cfg::Impulse::ExitOk | cfg::Impulse::ScanOk => 0,
            };
            terminate_all(code)
        }
    }
}

fn take_action(
    from: &cfg::WorkflowStep,
    impulse: &cfg::Impulse,
    app: &cfg::AppObj,
) -> anyhow::Result<()> {
    let links = from.links.as_ref();

    let link = match impulse {
        cfg::Impulse::ScanOk => links
            .and_then(|l| l.on_scan_ok.as_ref())
            .or_else(|| links.and_then(|l| l.on_ok.as_ref())),
        cfg::Impulse::ScanErr => links
            .and_then(|l| l.on_scan_err.as_ref())
            .or_else(|| links.and_then(|l| l.on_err.as_ref())),
        cfg::Impulse::ExitOk => links
            .and_then(|l| l.on_exit_ok.as_ref())
            .or_else(|| links.and_then(|l| l.on_ok.as_ref())),
        cfg::Impulse::ExitErr => links
            .and_then(|l| l.on_exit_err.as_ref())
            .or_else(|| links.and_then(|l| l.on_err.as_ref())),
    };

    let Some(link) = link else {
        return Ok(());
    };

    if from.final_step.unwrap_or(false) {
        return Ok(());
    }

    match resolve_link_action(link) {
        LinkAction::Step(step_name) => run_step_by_name(step_name, false, app),
        LinkAction::End => {
            let code = match impulse {
                cfg::Impulse::ExitErr | cfg::Impulse::ScanErr => 1,
                cfg::Impulse::ExitOk | cfg::Impulse::ScanOk => 0,
            };
            terminate_all(code)
        }
    }
}
