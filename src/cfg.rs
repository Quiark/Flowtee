use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxOpt {
    pub sess: String,
    pub win: String,
    #[serde(default)]
    pub fish_vi_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowOutputs {
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowLinkTyped {
    Step { step: String },
    End,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkflowLink {
    StepName(String),
    Typed(WorkflowLinkTyped),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowLinks {
    #[serde(default)]
    pub on_ok: Option<WorkflowLink>,
    #[serde(default)]
    pub on_err: Option<WorkflowLink>,

    #[serde(default)]
    pub on_scan_ok: Option<WorkflowLink>,
    #[serde(default)]
    pub on_scan_err: Option<WorkflowLink>,
    #[serde(default)]
    pub on_exit_ok: Option<WorkflowLink>,
    #[serde(default)]
    pub on_exit_err: Option<WorkflowLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    pub command: String,

    #[serde(default)]
    pub scan_ok: Option<String>,
    #[serde(default)]
    pub scan_err: Option<String>,

    #[serde(default)]
    pub pwd: Option<String>,
    #[serde(default)]
    pub outputs: Option<WorkflowOutputs>,

    #[serde(default)]
    pub tmux: Option<TmuxOpt>,
    #[serde(default)]
    pub links: Option<WorkflowLinks>,

    #[serde(default, rename = "final")]
    pub final_step: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub steps: Vec<WorkflowStep>,
}

pub struct AppObj {
    pub workflow: Workflow,
    pub workflow_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impulse {
    ScanOk,
    ScanErr,
    ExitOk,
    ExitErr,
}
