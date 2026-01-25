use std::process::Command;

/// Execute a tmux command without capturing output
///
/// # Arguments
/// * `args` - Arguments to pass to the tmux command
///
/// # Returns
/// Result containing the exit status
pub fn tmux(args: &[&str]) -> Result<std::process::ExitStatus, std::io::Error> {
    let mut cmd = Command::new("tmux");
    cmd.args(args);

    // Preserve current working directory
    if let Ok(cwd) = std::env::current_dir() {
        cmd.current_dir(cwd);
    }

    cmd.status()
}

/// Convenience function to send keys to a tmux session
///
/// # Arguments
/// * `target` - The target session, window, or pane (e.g., "session:0", "session:0.0")
/// * `keys` - The keys to send
///
/// # Returns
/// Result containing the exit status
pub fn tmux_send_keys(target: &str, keys: &str) -> Result<std::process::ExitStatus, std::io::Error> {
    tmux(&["send-keys", "-t", target, keys])
}

pub fn careful_run_command(target: &str, cmd: &str, fish_vim_mode: bool) -> anyhow::Result<()> {
    // cancel copy mode if any
    tmux(&["send-keys", "-t", target, "-X", "cancel"])?;
    // Send the command
    tmux_send_keys(target, cmd)?;
    // Send Enter to execute
    tmux_send_keys(target, "Enter")?;

    Ok(())
}
