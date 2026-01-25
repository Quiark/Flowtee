use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxOpt {
    pub sess: String,
    pub win: String,
    #[serde(default)]
    pub fish_vi_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowLinks {
    pub on_ok: Option<String>,
    pub on_err: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    pub command: String,
    pub scan_ok: String,
    #[serde(default)]
    pub scan_err: Option<String>,
    #[serde(default)]
    pub tmux: Option<TmuxOpt>,
    #[serde(default)]
    pub links: Option<WorkflowLinks>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub steps: Vec<WorkflowStep>,
}

pub struct AppObj {
    pub workflow: Workflow,
    pub workflow_name: String,
}

pub enum Impulse {
    Success,
    Error,
}
