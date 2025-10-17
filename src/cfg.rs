use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    pub command: String,
    pub scan_ok: String,
    #[serde(default)]
    pub scan_fail: Option<String>,
    #[serde(default)]
    pub tmux_sess: Option<String>,
    #[serde(default)]
    pub tmux_win: Option<String>,
    #[serde(default)]
    pub fish_vi_mode: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub steps: Vec<WorkflowStep>,
}
