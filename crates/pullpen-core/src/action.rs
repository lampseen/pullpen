//! What the user asks for, and what comes back (PLAN.md §3).

use crate::pr::{MergeMethod, PrId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Merge {
        method: MergeMethod,
    },
    Arm {
        method: MergeMethod,
    },
    Disarm,
    /// Turn the repo's auto-merge setting on, then arm.
    AllowAuto {
        method: MergeMethod,
    },
    Approve,
    Comment(String),
    Close,
    /// Bring the branch up to date with its base: GitHub merges the base in,
    /// GitLab rebases onto it, as their own buttons do.
    Update,
    /// Fetch the PR's head and switch to it as `branch` in the local clone.
    Checkout {
        branch: String,
    },
    /// Mark ready for review, or back to draft.
    Ready(bool),
    Reopen,
    /// A review that asks for changes, with what they are.
    RequestChanges(String),
    /// Ask these logins for a review, beside whoever was already asked.
    Review(Vec<String>),
    Label {
        add: Vec<String>,
        remove: Vec<String>,
    },
    /// Assign these logins, beside whoever already is.
    Assign(Vec<String>),
    Title(String),
    /// Run the failed jobs of the PR's newest pipeline again.
    Rerun,
    /// Stop the jobs of the PR's newest pipeline that have yet to finish.
    Cancel,
    Open,
    Refresh {
        all: bool,
    },
}

impl Action {
    /// The command that names this action, for messages: `:approve #2290: …`.
    pub fn command(&self) -> &'static str {
        match self {
            Action::Merge { .. } => "merge",
            Action::Arm { .. } | Action::Disarm | Action::AllowAuto { .. } => "automerge",
            Action::Approve => "approve",
            Action::Comment(_) => "comment",
            Action::Close => "close",
            Action::Update => "update",
            Action::Checkout { .. } => "checkout",
            Action::Ready(true) => "ready",
            Action::Ready(false) => "draft",
            Action::Reopen => "reopen",
            Action::RequestChanges(_) => "request-changes",
            Action::Review(_) => "review",
            Action::Label { .. } => "label",
            Action::Assign(_) => "assign",
            Action::Title(_) => "title",
            Action::Rerun => "rerun",
            Action::Cancel => "cancel",
            Action::Open => "open",
            Action::Refresh { .. } => "refresh",
        }
    }
}

/// One action on one PR, as the forge layer receives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: u64,
    pub target: PrId,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    /// The forge refused. `reason` is its explanation, in its words.
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobResult {
    pub job: Job,
    pub outcome: Outcome,
    /// What the forge or the local step did beyond the action itself, in
    /// words for the message: "source branch deleted", "in ~/src/api".
    pub note: Option<String>,
}
