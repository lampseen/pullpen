//! The `Forge` trait and its GitHub and GitLab clients (PLAN.md §4).

pub mod auth;
pub mod github;
pub mod gitlab;
mod http;

use std::collections::HashMap;

use async_trait::async_trait;
use pullpen_core::diff::FileDiff;
use pullpen_core::query::Query;
use pullpen_core::{
    Action, Capabilities, CheckState, Forge as Kind, Job, JobRun, MergeMethod, PipelineState, Pr, PrDetail, PrId, Run,
};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("not signed in to {0}; set a token or run its CLI's login")]
    Auth(String),
    /// The words are for an action, which nothing retries; the sync loop,
    /// which does retry, has its own.
    #[error("rate limited; try again in {retry_in}s")]
    RateLimited { retry_in: u64 },
    #[error("{host} {status}")]
    Server { host: String, status: u16 },
    /// The forge understood and said no. The text is the forge's own.
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Network(String),
    #[error("unexpected answer from the forge: {0}")]
    Decode(String),
}

impl Error {
    /// Errors that go on the status line and are retried with backoff, as
    /// opposed to ones a command reports (PLAN.md §4.2).
    pub fn transient(&self) -> bool {
        matches!(self, Error::RateLimited { .. } | Error::Server { .. } | Error::Network(_))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// One section's worth of PRs from one forge.
pub struct Page {
    pub prs: Vec<Pr>,
    /// What the forge says matched and pullpen may see, which can exceed `prs`.
    pub total: usize,
    /// Rows the forge counted but would not show, and why: the user should
    /// know their list is incomplete, not find out by noticing.
    pub hidden: Option<String>,
}

/// Somewhere PRs can be watched. The path is what searches take; the name
/// is what the forge's own pages show, which on GitLab can differ (a group
/// at `a5434` named "AIP").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub path: String,
    pub name: String,
}

/// Who counts as an agent: these logins, plus any login ending in `[bot]`
/// and any account the forge itself marks as a bot (PLAN.md §3).
#[derive(Clone, Debug, Default)]
pub struct Agents(pub Vec<String>);

impl Agents {
    pub fn default_logins() -> Vec<String> {
        ["claude", "codex", "renovate", "dependabot", "devin", "copilot", "github-actions"].map(String::from).to_vec()
    }
    pub fn is_agent(&self, login: &str, forge_says_bot: bool) -> bool {
        let bare = login.trim_end_matches("[bot]");
        forge_says_bot || login.ends_with("[bot]") || self.0.iter().any(|a| a.eq_ignore_ascii_case(bare))
    }
}

#[async_trait]
pub trait Forge: Send + Sync {
    fn kind(&self) -> Kind;
    fn host(&self) -> &str;
    /// The signed-in login, which is what `@me` means.
    async fn viewer(&self) -> Result<String>;
    /// Where the signed-in user can watch PRs: on GitHub their own account
    /// and their orgs, on GitLab the groups they belong to.
    async fn places(&self) -> Result<Vec<Place>>;
    /// PRs matching the server-side part of `q`; the caller applies the
    /// client-side terms.
    async fn search(&self, q: &Query, me: &str, limit: usize) -> Result<Page>;
    /// What each repo seen so far allows, keyed `host/repo`.
    fn capabilities(&self) -> HashMap<String, Capabilities>;
    /// Whether the source branch was deleted with it.
    async fn merge(&self, id: &PrId, m: MergeMethod) -> Result<bool>;
    async fn arm(&self, id: &PrId, m: MergeMethod) -> Result<()>;
    async fn disarm(&self, id: &PrId) -> Result<()>;
    /// Turns on the repo setting that `arm` needs.
    async fn allow_auto(&self, repo: &str) -> Result<()>;
    async fn approve(&self, id: &PrId) -> Result<()>;
    async fn comment(&self, id: &PrId, body: &str) -> Result<()>;
    async fn close(&self, id: &PrId) -> Result<()>;
    /// Brings the branch up to date with its base.
    async fn update(&self, id: &PrId) -> Result<()>;
    /// Description, checks and timeline, for the preview and detail view.
    async fn detail(&self, id: &PrId) -> Result<PrDetail>;
    async fn diff(&self, id: &PrId) -> Result<Vec<FileDiff>>;
    /// The PR's pipeline runs: after its merge, if merged, then its head's.
    async fn runs(&self, id: &PrId) -> Result<Vec<Run>>;
    /// A job's log so far, or its tail when long.
    async fn log(&self, id: &PrId, job: &str) -> Result<String>;
    async fn ready(&self, id: &PrId, ready: bool) -> Result<()>;
    async fn reopen(&self, id: &PrId) -> Result<()>;
    async fn request_changes(&self, id: &PrId, body: &str) -> Result<()>;
    /// Adds reviewers; those already asked stay asked.
    async fn request_review(&self, id: &PrId, logins: &[String]) -> Result<()>;
    /// The PR's labels afterwards.
    async fn label(&self, id: &PrId, add: &[String], remove: &[String]) -> Result<Vec<String>>;
    /// Adds assignees; those already assigned stay.
    async fn assign(&self, id: &PrId, logins: &[String]) -> Result<()>;
    async fn retitle(&self, id: &PrId, title: &str) -> Result<()>;
    /// Opens a PR and answers with its URL. No `base` means the repo's
    /// default branch.
    async fn create(&self, repo: &str, head: &str, base: Option<&str>, title: &str, body: &str) -> Result<String>;
    /// Runs these jobs of the PR's pipeline again.
    async fn rerun(&self, id: &PrId, jobs: &[JobRun]) -> Result<()>;
    /// Stops these jobs of the PR's pipeline.
    async fn cancel(&self, id: &PrId, jobs: &[JobRun]) -> Result<()>;
}

/// Each (repo, base branch) that a merge failed on, once: whether the branch
/// passes now decides if that failure still needs anyone (ADR-019).
pub fn failed_branches(prs: &[Pr]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for p in prs.iter().filter(|p| p.post_merge.as_ref().is_some_and(|x| x.state == PipelineState::Failed)) {
        let key = (p.id.repo.clone(), p.base.clone());
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// Records what a base branch's newest pipeline did on each failed row of it.
pub fn set_branch(prs: &mut [Pr], (repo, base): &(String, String), state: PipelineState) {
    for p in prs.iter_mut().filter(|p| &p.id.repo == repo && &p.base == base) {
        if let Some(pl) = p.post_merge.as_mut().filter(|x| x.state == PipelineState::Failed) {
            pl.branch = Some(state);
        }
    }
}

/// Runs a job's action, and says what else happened. Opening and checking
/// out are local, not forge calls.
pub async fn run(f: &dyn Forge, job: &Job) -> Result<Option<String>> {
    let id = &job.target;
    let done = match &job.action {
        Action::Merge { method } => {
            return Ok(f.merge(id, *method).await?.then(|| "source branch deleted".to_string()));
        }
        Action::Arm { method } => f.arm(id, *method).await,
        Action::Disarm => f.disarm(id).await,
        Action::AllowAuto { method } => {
            f.allow_auto(&id.repo).await?;
            f.arm(id, *method).await
        }
        Action::Approve => f.approve(id).await,
        Action::Comment(body) => f.comment(id, body).await,
        Action::Close => f.close(id).await,
        Action::Update => f.update(id).await,
        Action::Ready(ready) => f.ready(id, *ready).await,
        Action::Reopen => f.reopen(id).await,
        Action::RequestChanges(body) => f.request_changes(id, body).await,
        Action::Review(logins) => f.request_review(id, logins).await,
        Action::Label { add, remove } => {
            let now = f.label(id, add, remove).await?;
            return Ok(Some(if now.is_empty() { "no labels now".into() } else { format!("now {}", now.join(", ")) }));
        }
        Action::Assign(logins) => f.assign(id, logins).await,
        Action::Title(title) => f.retitle(id, title).await,
        Action::Create { head, base, title, body } => {
            return f.create(&id.repo, head, base.as_deref(), title, body).await.map(Some);
        }
        Action::Rerun | Action::Cancel => return pipeline(f, job).await.map(Some),
        Action::Checkout { .. } | Action::Open | Action::Refresh { .. } => Ok(()),
    };
    done.map(|()| None)
}

/// Re-runs what failed, or cancels what has yet to finish, in the PR's
/// newest pipeline: the one after its merge if it has merged. Says how many
/// jobs that was. A check from outside the forge's CI cannot be asked.
async fn pipeline(f: &dyn Forge, job: &Job) -> Result<String> {
    let rerun = job.action == Action::Rerun;
    let wanted = |j: &&JobRun| match j.state {
        CheckState::Failed => rerun,
        CheckState::Running | CheckState::Queued => !rerun,
        CheckState::Passed => false,
    };
    let runs = f.runs(&job.target).await?;
    let jobs: Vec<JobRun> = runs
        .first()
        .into_iter()
        .flat_map(|r| &r.jobs)
        .filter(|j| j.log && !j.id.is_empty())
        .filter(wanted)
        .cloned()
        .collect();
    if jobs.is_empty() {
        let none = if rerun { "no failed jobs to run again" } else { "no jobs still running" };
        return Err(Error::Refused(none.into()));
    }
    if rerun {
        f.rerun(&job.target, &jobs).await?
    } else {
        f.cancel(&job.target, &jobs).await?
    }
    Ok(if jobs.len() == 1 { jobs[0].name.clone() } else { format!("{} jobs", jobs.len()) })
}

#[cfg(test)]
mod tests;
