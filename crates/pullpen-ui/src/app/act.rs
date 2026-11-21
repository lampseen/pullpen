//! Acting on PRs: who is eligible, what to confirm, the jobs that run, and
//! what the command line says about them (design §05–§07).

use std::collections::VecDeque;

use pullpen_core::{
    Action, AutoMerge, Block, Job, JobResult, MergeMethod, Outcome, Pr, PrId, Readiness, Verdict, Wait, verdict,
};

use super::{App, DefaultMethod, Effect, Input};
use crate::keymap::Key;
use crate::palette::Tok;
use crate::span::{Line, fit, lr, markup, s, sp, with_bg};
use crate::views::chrome::Mode;
use crate::views::help::PANEL_W;

/// Markup has no escape; a brace in a title or a forge message would open a
/// tag. Full-width braces read the same and cannot.
pub fn esc(t: &str) -> String {
    t.replace('{', "｛").replace('}', "｝")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Key,
    /// Typed on `:`. A failure keeps the line in COMMAND so it can be fixed.
    Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Merge {
        method: MergeMethod,
        allowed: Vec<MergeMethod>,
    },
    /// Several PRs at once, which always confirms.
    Arm {
        method: MergeMethod,
        allowed: Vec<MergeMethod>,
    },
    /// One PR, a repo that allows several methods, and none configured.
    ArmMethod {
        default: MergeMethod,
        allowed: Vec<MergeMethod>,
    },
    /// `M` on a PR that is only waiting: offer auto-merge instead.
    ArmInstead,
    /// `A` on a PR the forge would merge at once and so will not arm.
    MergeNow {
        method: MergeMethod,
    },
    /// `A` in a repo whose auto-merge setting is off, asked of its admin.
    AllowAuto {
        method: MergeMethod,
    },
    Close,
    Approve,
    Comment(String),
    Update,
    /// `:new`, which always asks: the PR is made of what the clone says.
    New(Action),
    /// Any other action on several PRs at once, which asks once.
    Several(Action),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub targets: Vec<PrId>,
    pub origin: Origin,
    /// The command as typed, for putting back on the line after a failure.
    pub typed: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Running {
    pub job: Job,
    pub batch: Option<usize>,
    pub origin: Origin,
    pub typed: Option<String>,
    /// An arm on a PR that was already green, run as a merge.
    pub merging_now: bool,
    /// The auto-merge state before an optimistic arm or disarm, restored if
    /// the forge refuses.
    pub before: Option<Option<AutoMerge>>,
}

#[derive(Clone, Debug, Default)]
pub struct Batch {
    pub action: Option<Action>,
    pub queue: VecDeque<Job>,
    pub running: usize,
    pub done: usize,
    pub merging_now: Vec<u64>,
    pub failed: Vec<(u64, String)>,
    pub skipped: Vec<(u64, String)>,
    pub reported: bool,
}

const ORDER: [MergeMethod; 3] = [MergeMethod::Squash, MergeMethod::Merge, MergeMethod::Rebase];

fn key_of(m: MergeMethod) -> char {
    match m {
        MergeMethod::Squash => 's',
        MergeMethod::Merge => 'm',
        MergeMethod::Rebase => 'r',
    }
}

fn method_of(c: char) -> Option<MergeMethod> {
    ORDER.into_iter().find(|m| key_of(*m) == c)
}

/// How GitHub's refusal of an update with nothing to bring in reads.
const UP_TO_DATE: &str = "no new commits on the base branch";

/// What the progress line says while a job runs: "Merging #812 on …".
pub fn verb(a: &Action) -> &'static str {
    match a {
        Action::Merge { .. } => "Merging",
        Action::Arm { .. } | Action::AllowAuto { .. } => "Arming auto-merge on",
        Action::Disarm => "Disarming auto-merge on",
        Action::Approve => "Approving",
        Action::Comment(_) => "Commenting on",
        Action::Close => "Closing",
        Action::Update => "Updating",
        Action::Rerun => "Re-running failed jobs of",
        Action::Create { .. } => "Opening a PR in",
        Action::Label { .. } => "Labelling",
        Action::Assign(_) => "Assigning",
        Action::Title(_) => "Retitling",
        Action::Ready(true) => "Marking ready",
        Action::Ready(false) => "Putting back to draft",
        Action::Reopen => "Reopening",
        Action::RequestChanges(_) => "Requesting changes on",
        Action::Review(_) => "Requesting a review of",
        Action::Cancel => "Cancelling the pipeline of",
        Action::Checkout { .. } => "Checking out",
        Action::Open => "Opening",
        Action::Refresh { .. } => "Refreshing",
    }
}

/// What a job's target is called: its number, or its repo for a PR that
/// does not exist yet.
pub fn name(id: &PrId) -> String {
    if id.number == 0 { id.repo.clone() } else { format!("#{}", id.number) }
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") }
}

fn people(p: &[String]) -> String {
    p.iter().map(|x| format!("@{x}")).collect::<Vec<_>>().join(", ")
}

/// Why a waiting PR is not ready, for the arm-instead prompt.
fn not_ready(waits: &[Wait]) -> String {
    waits
        .iter()
        .map(|w| match w {
            Wait::Ci { passed, total } => format!("CI {passed}/{total} running"),
            Wait::Reviews { have, need, .. } => format!("{} required", plural((need - have) as usize, "review")),
            Wait::Train { position, length } => format!("merge train position {position} of {length}"),
            Wait::Behind { base } => format!("behind {base}"),
            Wait::Mergeability => "mergeability not known yet".into(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// What an armed PR is waiting on, for the armed message.
fn waiting_on(waits: &[Wait]) -> String {
    let parts: Vec<String> = waits
        .iter()
        .map(|w| match w {
            Wait::Ci { passed, total } => format!("CI ({passed}/{total})"),
            Wait::Reviews { have, need, pending } if pending.is_empty() => plural((need - have) as usize, "review"),
            Wait::Reviews { have, need, pending } => {
                format!("{} from {}", plural((need - have) as usize, "review"), people(pending))
            }
            Wait::Train { position, length } => format!("the merge train ({position} of {length})"),
            Wait::Behind { base } => format!("an update from {base}"),
            Wait::Mergeability => "the mergeability check".into(),
        })
        .collect();
    match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        n => format!("{} and {}", parts[..n - 1].join(", "), parts[n - 1]),
    }
}

/// Everything that stops a merge, and the key that helps with the first.
fn blockers(p: &Pr) -> (Vec<String>, &'static str) {
    let mut why = Vec::new();
    let mut hint = "o open in browser";
    if p.draft {
        why.push("draft".to_string());
    }
    if p.mergeable == pullpen_core::Mergeable::Conflict {
        why.push(format!("conflicts with {}", p.base));
        hint = ":checkout to resolve";
    }
    for c in &p.checks.failed {
        why.push(format!("{} failing", c.name));
        if hint == "o open in browser" {
            hint = "d diff   o open in browser";
        }
    }
    if p.reviews.changes_requested {
        why.push("changes requested".into());
    }
    if p.reviews.short() {
        why.push(format!("{} required", plural((p.reviews.need - p.reviews.have) as usize, "review")));
    }
    if !p.checks.failing() && !p.checks.all_passed() {
        why.push(format!("CI {}/{} running", p.checks.required_passed, p.checks.required_total));
    }
    (why, hint)
}

/// The one-word reason a PR is skipped in a batch.
fn skip_reason(p: &Pr, r: Readiness) -> String {
    match r {
        Readiness::Merged => "merged".into(),
        Readiness::Draft => "draft".into(),
        Readiness::Conflict => "conflict".into(),
        Readiness::Failing => {
            p.checks.failed.first().map_or("changes requested".into(), |c| format!("{} failing", c.name))
        }
        Readiness::Waiting => "not ready".into(),
        Readiness::Ready => String::new(),
    }
}

fn alts(method: MergeMethod, allowed: &[MergeMethod], merge_word: &str) -> String {
    ORDER
        .iter()
        .filter(|m| **m != method && allowed.contains(m))
        .map(|m| match m {
            MergeMethod::Merge => format!("m {merge_word}"),
            other => format!("{} {}", key_of(*other), other.word()),
        })
        .map(|t| format!("   {t}"))
        .collect()
}

pub fn messages_panel(log: &VecDeque<Line>, h: usize) -> Vec<Line> {
    let pad = |l: &[crate::span::Span]| {
        let mut v = vec![sp(3)];
        v.extend_from_slice(l);
        with_bg(fit(&v, PANEL_W, true), Tok::Surf)
    };
    let room = h.saturating_sub(12).max(1);
    let mut out = vec![pad(&[]), pad(&lr(&markup("{B|messages}"), &markup("{d|Esc to close}"), PANEL_W - 6)), pad(&[])];
    if log.is_empty() {
        out.push(pad(&markup("{d|nothing yet}")));
    }
    for l in log.iter().skip(log.len().saturating_sub(room)) {
        out.push(pad(&fit(l, PANEL_W - 6, true)));
    }
    out.push(pad(&[]));
    out
}

impl App {
    /// An explicit target, else the marks, else the selected row.
    pub fn targets(&self, explicit: Option<PrId>) -> Vec<PrId> {
        if let Some(t) = explicit {
            return vec![t];
        }
        let s = self.section();
        if !s.marks.is_empty() {
            return s.marks.clone();
        }
        s.selected().map(|p| vec![p.id.clone()]).unwrap_or_default()
    }

    fn default_method(&self, p: &Pr) -> MergeMethod {
        match self.ui.default_method {
            // A configured method is a preference across repos: one that
            // does not allow it gets its own default, not a refusal.
            DefaultMethod::Fixed(m) if p.repo_allowed_methods.is_empty() || p.repo_allowed_methods.contains(&m) => m,
            _ => p.repo_default_method,
        }
    }

    /// Says so, and returns true, when a method named on the command line is
    /// one the repo does not allow.
    fn disallowed(&mut self, p: &Pr, method: Option<MergeMethod>, mode: Mode) -> bool {
        let Some(m) = method.filter(|m| !p.repo_allowed_methods.is_empty() && !p.repo_allowed_methods.contains(m))
        else {
            return false;
        };
        let allowed: Vec<&str> = p.repo_allowed_methods.iter().map(|m| m.word()).collect();
        let line = markup(&format!(
            "{{r|E  {} doesn't allow {}}}{{d|      it allows {}}}",
            esc(&p.id.repo),
            m.word(),
            allowed.join(", ")
        ));
        self.say(line, mode);
        true
    }

    /// Says so, and returns true, when the PR waits on nothing but being
    /// behind its base: auto-merge would wait on an update nobody asked
    /// for, so name the key that makes one.
    fn only_behind(&mut self, p: &Pr, mode: Mode) -> bool {
        let behind = matches!(verdict(p, &self.caps_of(p)), Verdict::Waiting(w) if w.iter().all(|x| matches!(x, Wait::Behind { .. })));
        if behind {
            let line = markup(&format!(
                "{{r|E  #{} is behind {}}}{{d|      u updates the branch}}",
                p.id.number,
                esc(&p.base)
            ));
            self.say(line, mode);
        }
        behind
    }

    fn allowed(&self, ids: &[PrId]) -> Vec<MergeMethod> {
        ORDER
            .into_iter()
            .filter(|m| ids.iter().filter_map(|id| self.find(id)).all(|p| p.repo_allowed_methods.contains(m)))
            .collect()
    }

    fn ask(&mut self, kind: PromptKind, targets: Vec<PrId>, origin: Origin, typed: Option<String>) {
        self.input = Input::Confirm(Prompt { kind, targets, origin, typed });
    }

    pub fn request_merge(
        &mut self,
        ids: Vec<PrId>,
        method: Option<MergeMethod>,
        origin: Origin,
        typed: Option<String>,
    ) {
        let Some(first) = ids.first().and_then(|id| self.find(id)).cloned() else { return };
        let mode = if origin == Origin::Command { Mode::Command } else { Mode::Normal };
        if self.disallowed(&first, method, mode) {
            return;
        }
        let method = method.unwrap_or_else(|| self.default_method(&first));
        if ids.len() > 1 {
            let allowed = self.allowed(&ids);
            return self.ask(PromptKind::Merge { method, allowed }, ids, origin, typed);
        }
        match verdict(&first, &self.caps_of(&first)) {
            _ if first.merged() => self.error(&format!("#{} is already merged", first.id.number), mode),
            Verdict::Ready => {
                let allowed = first.repo_allowed_methods.clone();
                self.ask(PromptKind::Merge { method, allowed }, ids, origin, typed)
            }
            _ if self.only_behind(&first, mode) => {}
            Verdict::Waiting(_) => self.ask(PromptKind::ArmInstead, ids, origin, typed),
            Verdict::Blocked(_) => {
                let (why, hint) = blockers(&first);
                let line = markup(&format!(
                    "{{r|E  #{} can't merge: {}}}{{d|      {hint}}}",
                    first.id.number,
                    esc(&why.join(" · "))
                ));
                self.say(line, mode);
            }
        }
    }

    /// `A`: disarms an armed PR, merges a green one, and otherwise arms,
    /// asking for a method only when the repo leaves it open.
    pub fn request_arm(
        &mut self,
        ids: Vec<PrId>,
        method: Option<MergeMethod>,
        origin: Origin,
        typed: Option<String>,
    ) -> Vec<Effect> {
        let Some(first) = ids.first().and_then(|id| self.find(id)).cloned() else { return vec![] };
        let mode = if origin == Origin::Command { Mode::Command } else { Mode::Normal };
        if self.disallowed(&first, method, mode) {
            return vec![];
        }
        if ids.len() > 1 {
            let method = method.unwrap_or_else(|| self.default_method(&first));
            let allowed = self.allowed(&ids);
            self.ask(PromptKind::Arm { method, allowed }, ids, origin, typed);
            return vec![];
        }
        if let Some(a) = &first.auto_merge
            && method.is_none_or(|m| m == a.method)
        {
            return self.start(vec![(first.id.clone(), Action::Disarm, false)], None, origin, typed);
        }
        match self.readiness(&first) {
            Readiness::Merged => {
                self.error(&format!("#{} is already merged", first.id.number), mode);
                vec![]
            }
            Readiness::Draft => {
                self.error(&format!("#{} can't arm auto-merge: draft", first.id.number), mode);
                vec![]
            }
            Readiness::Ready => {
                let m = method.unwrap_or_else(|| self.default_method(&first));
                self.start(vec![(first.id.clone(), Action::Merge { method: m }, true)], None, origin, typed)
            }
            _ if self.only_behind(&first, mode) => vec![],
            _ if first.mergeable_now => {
                let method = method.unwrap_or_else(|| self.default_method(&first));
                self.ask(PromptKind::MergeNow { method }, ids, origin, typed);
                vec![]
            }
            // The forge refuses every arm in such a repo, so nothing is sent
            // that cannot work; an admin is offered the setting instead.
            _ if self.caps_of(&first).auto_merge_off => {
                if self.caps_of(&first).can_admin {
                    let method = method.unwrap_or_else(|| self.default_method(&first));
                    self.ask(PromptKind::AllowAuto { method }, ids, origin, typed);
                } else {
                    let line = markup(&format!(
                        "{{r|E  #{} can't arm: {} has auto-merge off}}{{d|      a repo admin allows it in the repo's settings}}",
                        first.id.number,
                        esc(&first.id.repo)
                    ));
                    self.say(line, mode);
                }
                vec![]
            }
            _ => match (method, self.ui.default_method) {
                (Some(_), _) | (None, DefaultMethod::Fixed(_)) => {
                    let m = method.unwrap_or_else(|| self.default_method(&first));
                    self.start(vec![(first.id.clone(), Action::Arm { method: m }, false)], None, origin, typed)
                }
                (None, DefaultMethod::Repo) if first.repo_allowed_methods.len() > 1 => {
                    let kind = PromptKind::ArmMethod {
                        default: first.repo_default_method,
                        allowed: first.repo_allowed_methods.clone(),
                    };
                    self.ask(kind, ids, origin, typed);
                    vec![]
                }
                (None, DefaultMethod::Repo) => {
                    let m = first.repo_allowed_methods.first().copied().unwrap_or(first.repo_default_method);
                    self.start(vec![(first.id.clone(), Action::Arm { method: m }, false)], None, origin, typed)
                }
            },
        }
    }

    pub fn request_simple(
        &mut self,
        ids: Vec<PrId>,
        action: Action,
        origin: Origin,
        typed: Option<String>,
    ) -> Vec<Effect> {
        // A second update while the first is under way is refused by the
        // forge, and its refusal would replace the first one's answer.
        if let ([id], Action::Update) = (&ids[..], &action)
            && (self.updating.contains_key(id)
                || self.jobs.iter().any(|j| j.job.target == *id && j.job.action == Action::Update))
        {
            self.say(markup(&format!("{{d|#{} is already being updated}}", id.number)), Mode::Normal);
            return vec![];
        }
        if let ([id], Action::Update) = (&ids[..], &action)
            && self.find(id).is_some_and(|p| p.merged())
        {
            self.error(&format!("#{} is already merged", id.number), Mode::Normal);
            return vec![];
        }
        let kind = match &action {
            Action::Close => Some(PromptKind::Close),
            Action::Update if ids.len() > 1 => Some(PromptKind::Update),
            Action::Approve if ids.len() > 1 => Some(PromptKind::Approve),
            Action::Comment(t) if ids.len() > 1 => Some(PromptKind::Comment(t.clone())),
            a if ids.len() > 1 => Some(PromptKind::Several(a.clone())),
            _ => None,
        };
        match kind {
            Some(k) => {
                self.ask(k, ids, origin, typed);
                vec![]
            }
            None => {
                let jobs = ids.into_iter().map(|id| (id, action.clone(), false)).collect();
                self.start(jobs, None, origin, typed)
            }
        }
    }

    /// One key at a confirm prompt. Anything but a listed key cancels and
    /// is discarded (design §07).
    pub fn confirm_key(&mut self, key: Key) -> Vec<Effect> {
        let Input::Confirm(p) = std::mem::take(&mut self.input) else { return vec![] };
        let pick = |c: char, allowed: &[MergeMethod]| method_of(c).filter(|m| allowed.contains(m));
        let (origin, typed, ids) = (p.origin, p.typed.clone(), p.targets.clone());
        match (&p.kind, key) {
            (PromptKind::Merge { method, .. }, Key::Char('y') | Key::Enter) => {
                self.run_batch(ids, Action::Merge { method: *method }, origin, typed)
            }
            (PromptKind::Merge { allowed, .. }, Key::Char(c)) if pick(c, allowed).is_some() => {
                let m = pick(c, allowed).unwrap_or(MergeMethod::Squash);
                self.run_batch(ids, Action::Merge { method: m }, origin, typed)
            }
            (PromptKind::Arm { method, .. }, Key::Char('y') | Key::Enter) => {
                self.run_batch(ids, Action::Arm { method: *method }, origin, typed)
            }
            (PromptKind::Arm { allowed, .. }, Key::Char(c)) if pick(c, allowed).is_some() => {
                let m = pick(c, allowed).unwrap_or(MergeMethod::Squash);
                self.run_batch(ids, Action::Arm { method: m }, origin, typed)
            }
            (PromptKind::ArmMethod { default, .. }, Key::Enter) => {
                let id = ids[0].clone();
                self.start(vec![(id, Action::Arm { method: *default }, false)], None, origin, typed)
            }
            (PromptKind::ArmMethod { allowed, .. }, Key::Char(c)) if pick(c, allowed).is_some() => {
                let m = pick(c, allowed).unwrap_or(MergeMethod::Squash);
                self.start(vec![(ids[0].clone(), Action::Arm { method: m }, false)], None, origin, typed)
            }
            (PromptKind::MergeNow { method }, Key::Char('y') | Key::Enter) => {
                self.start(vec![(ids[0].clone(), Action::Merge { method: *method }, false)], None, origin, typed)
            }
            (PromptKind::AllowAuto { method }, Key::Char('y') | Key::Enter) => {
                self.start(vec![(ids[0].clone(), Action::AllowAuto { method: *method }, false)], None, origin, typed)
            }
            (PromptKind::ArmInstead, Key::Char('y' | 'A')) => self.request_arm(ids, None, origin, typed),
            (PromptKind::Close, Key::Char('y') | Key::Enter) => self.run_batch(ids, Action::Close, origin, typed),
            (PromptKind::Update, Key::Char('y') | Key::Enter) => self.run_batch(ids, Action::Update, origin, typed),
            (PromptKind::Approve, Key::Char('y') | Key::Enter) => self.run_batch(ids, Action::Approve, origin, typed),
            (PromptKind::Several(a), Key::Char('y') | Key::Enter) => self.run_batch(ids, a.clone(), origin, typed),
            (PromptKind::New(a), Key::Char('y') | Key::Enter) => {
                self.start(vec![(ids[0].clone(), a.clone(), false)], None, origin, typed)
            }
            (PromptKind::Comment(t), Key::Char('y') | Key::Enter) => {
                let t = t.clone();
                self.run_batch(ids, Action::Comment(t), origin, typed)
            }
            _ => vec![],
        }
    }

    /// Splits targets into jobs, skips the ineligible by name, and starts.
    fn run_batch(&mut self, ids: Vec<PrId>, action: Action, origin: Origin, typed: Option<String>) -> Vec<Effect> {
        if ids.len() == 1 && !matches!(action, Action::Arm { .. }) {
            return self.start(vec![(ids[0].clone(), action, false)], None, origin, typed);
        }
        let mut jobs = Vec::new();
        let mut skipped = Vec::new();
        for id in &ids {
            let Some(p) = self.find(id).cloned() else { continue };
            let r = self.readiness(&p);
            match &action {
                Action::Merge { .. } if r != Readiness::Ready => skipped.push((p.id.number, skip_reason(&p, r))),
                Action::Arm { .. } if matches!(r, Readiness::Draft | Readiness::Merged) => {
                    skipped.push((p.id.number, skip_reason(&p, r)))
                }
                Action::Arm { method } if r == Readiness::Ready => {
                    jobs.push((p.id.clone(), Action::Merge { method: *method }, true))
                }
                // Arming it would be refused, and merging a row that is not
                // green is not what a batch arm was asked for.
                Action::Arm { .. } if p.mergeable_now => {
                    skipped.push((p.id.number, "can merge now: A on its own".into()))
                }
                // A batch never changes a repo's settings.
                Action::Arm { .. } if self.caps_of(&p).auto_merge_off => {
                    skipped.push((p.id.number, "repo has auto-merge off".into()))
                }
                Action::Arm { method } if p.auto_merge.as_ref().is_some_and(|a| a.method == *method) => {
                    skipped.push((p.id.number, "already armed".into()))
                }
                _ => jobs.push((p.id.clone(), action.clone(), false)),
            }
        }
        self.section_mut().marks.clear();
        let b = self.batches.len();
        self.batches.push(Batch { action: Some(action), skipped, ..Default::default() });
        self.start(jobs, Some(b), origin, typed)
    }

    /// Starts jobs. In a batch, regular jobs run one at a time and an
    /// already-green merge starts at once, so the batch reports when the
    /// arming is done rather than waiting on the merge.
    pub fn start(
        &mut self,
        jobs: Vec<(PrId, Action, bool)>,
        batch: Option<usize>,
        origin: Origin,
        typed: Option<String>,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        for (target, action, merging_now) in jobs {
            let job = Job { id: self.next_job, target, action };
            self.next_job += 1;
            let queued = match batch {
                Some(b) if !merging_now => {
                    let bt = &mut self.batches[b];
                    let wait = bt.running > 0;
                    if wait {
                        bt.queue.push_back(job.clone());
                    } else {
                        bt.running += 1;
                    }
                    wait
                }
                _ => false,
            };
            if merging_now && let Some(b) = batch {
                self.batches[b].merging_now.push(job.target.number);
            }
            // Read before the optimistic ◆ goes on: this is what a refusal
            // puts back.
            let before = self.find(&job.target).map(|p| p.auto_merge.clone());
            self.optimistic(&job, merging_now);
            let run = Running { job: job.clone(), batch, origin, typed: typed.clone(), merging_now, before };
            self.jobs.push(run);
            if !queued {
                effects.push(Effect::Run(job));
            }
        }
        if let Some(b) = batch {
            self.report_batch(b);
        }
        effects
    }

    /// Arming shows ◆ at once; the forge's answer confirms or reverts it.
    fn optimistic(&mut self, job: &Job, merging_now: bool) {
        let me = self.me.clone();
        let now = self.now;
        let am = match &job.action {
            Action::Arm { method } | Action::AllowAuto { method } => {
                Some(AutoMerge { method: *method, armed_by: me, armed_at: now })
            }
            Action::Merge { method } if merging_now => Some(AutoMerge { method: *method, armed_by: me, armed_at: now }),
            _ => return,
        };
        self.each_pr(&job.target, |p| p.auto_merge = am.clone());
    }

    fn each_pr(&mut self, id: &PrId, mut f: impl FnMut(&mut Pr)) {
        for s in &mut self.sections {
            for p in s.prs.iter_mut().filter(|p| p.id == *id) {
                f(p);
            }
        }
    }

    /// Drops a merged or closed PR from every section. The cursor stays at
    /// the same index (design §05).
    fn remove_pr(&mut self, id: &PrId) {
        self.ended.insert(id.clone(), self.now);
        for s in &mut self.sections {
            let before = s.prs.len();
            s.prs.retain(|p| p.id != *id);
            if s.prs.len() < before {
                s.total = s.total.saturating_sub(1);
            }
            s.marks.retain(|m| m != id);
            s.sel = s.sel.min(s.visible().len().saturating_sub(1));
        }
        // A detail view of a PR that merged or closed has nothing to show.
        if self.detail.as_ref().is_some_and(|d| d.id == *id) {
            self.detail = None;
        }
        // A prompt asked about a PR that has since left: it is no longer a
        // question anyone can answer.
        if let Input::Confirm(p) = &mut self.input {
            p.targets.retain(|t| t != id);
            if p.targets.is_empty() {
                self.input = Input::Normal;
            }
        }
    }

    pub fn job_done(&mut self, r: JobResult) -> Vec<Effect> {
        let Some(i) = self.jobs.iter().position(|j| j.job.id == r.job.id) else { return vec![] };
        let run = self.jobs.remove(i);
        let id = run.job.target.clone();
        let n = id.number;
        let single = run.batch.is_none() || run.merging_now;
        if r.outcome == Outcome::Done && matches!(run.job.action, Action::Merge { .. } | Action::Disarm | Action::Close)
        {
            self.unwatch(&id);
        }
        match &r.outcome {
            Outcome::Done => {
                if let Some(line) = self.apply_done(&run, r.note.as_deref())
                    && single
                {
                    self.say(line, Mode::Normal);
                }
                if let Some(b) = run.batch.filter(|_| !run.merging_now) {
                    self.batches[b].done += 1;
                }
            }
            Outcome::Failed { reason } => {
                if let Some(before) = &run.before {
                    let before = before.clone();
                    self.each_pr(&id, |p| p.auto_merge = before.clone());
                }
                match run.batch.filter(|_| !run.merging_now) {
                    Some(b) => self.batches[b].failed.push((n, reason.clone())),
                    // GitHub refuses an update with nothing to bring in.
                    // That is an answer to `u`, not a failure of it.
                    None if run.job.action == Action::Update && reason.contains(UP_TO_DATE) => {
                        self.say(markup(&format!("{{d|#{n} is already up to date with its base}}")), Mode::Normal);
                    }
                    None => {
                        let text = format!(":{} {}: {reason}", run.job.action.command(), name(&id));
                        let to_line = run.origin == Origin::Command && matches!(self.input, Input::Normal);
                        if to_line {
                            self.input =
                                Input::CommandError { text: run.typed.clone().unwrap_or_default(), suggestion: None };
                        }
                        self.error(&text, if to_line { Mode::Command } else { Mode::Normal });
                    }
                }
            }
        }
        let mut effects = Vec::new();
        if let Some(b) = run.batch.filter(|_| !run.merging_now) {
            self.batches[b].running = self.batches[b].running.saturating_sub(1);
            if let Some(next) = self.batches[b].queue.pop_front() {
                self.batches[b].running += 1;
                effects.push(Effect::Run(next));
            }
            self.report_batch(b);
        }
        effects
    }

    /// Applies a finished job to the rows, and says so.
    fn apply_done(&mut self, run: &Running, note: Option<&str>) -> Option<Line> {
        let id = &run.job.target;
        let n = id.number;
        if let Action::Create { .. } = run.job.action {
            return Some(markup(&format!("{{g|✓}} {{f|Opened {}}}", esc(note.unwrap_or_default()))));
        }
        // A sync can drop the row while its action runs. The action still
        // happened, and saying nothing reads as though it had not.
        let Some(p) = self.find(id).cloned() else {
            let cmd = run.job.action.command();
            let local = matches!(run.job.action, Action::Open | Action::Refresh { .. });
            return (!local).then(|| markup(&format!("{{g|✓}} {{f|:{cmd} #{n} done}}{{d|   it has left this list}}")));
        };
        Some(match &run.job.action {
            Action::Merge { method } => {
                self.remove_pr(id);
                let note = note.map_or(String::new(), |n| format!(" · {}", esc(n)));
                markup(&format!("{{g|✓}} {{f|Merged #{n} into {}}}{{d|   {}{note}}}", esc(&p.base), method.word()))
            }
            Action::Arm { method } | Action::AllowAuto { method } => {
                let wait = match verdict(&p, &self.caps_of(&p)) {
                    Verdict::Waiting(w) => format!("waiting on {}", waiting_on(&w)),
                    Verdict::Blocked(Block::Failing { check }) => format!("merges once {check} passes"),
                    Verdict::Blocked(_) => "blocked until the PR is fixed".into(),
                    Verdict::Ready => "merging now".into(),
                };
                markup(&format!("{{a|◆}} {{f|Armed #{n}}}{{d|   {} · {}}}", method.word(), esc(&wait)))
            }
            Action::Disarm => {
                self.each_pr(id, |p| p.auto_merge = None);
                markup(&format!("{{f|Auto-merge off for #{n}}}{{d|   A to re-arm}}"))
            }
            // The row keeps the forge's count until the next sync; only arming
            // is shown before the forge confirms it.
            Action::Approve => {
                let armed = if p.armed() { "auto-merge armed" } else { "auto-merge not armed" };
                markup(&format!(
                    "{{g|✓}} {{f|Approved #{n}}}{{d|   {} · {} of {} required ({armed})}}",
                    esc(&p.id.repo),
                    p.reviews.have + 1,
                    p.reviews.need
                ))
            }
            Action::Comment(_) => markup(&format!("{{g|✓}} {{f|Commented on #{n}}}")),
            Action::Close => {
                self.remove_pr(id);
                markup(&format!("{{g|✓}} {{f|Closed #{n}}}{{d|   without merging}}"))
            }
            Action::Update => {
                // The forge rewrites the branch; until the next sync says how
                // it stands, its mergeability is not known.
                self.each_pr(id, |p| p.mergeable = pullpen_core::Mergeable::Unknown);
                self.updating.insert(id.clone(), self.now);
                let how = match id.forge {
                    pullpen_core::Forge::GitHub => format!("merged {} into it", esc(&p.base)),
                    pullpen_core::Forge::GitLab => format!("rebased onto {}", esc(&p.base)),
                };
                markup(&format!("{{g|✓}} {{f|Updated #{n}}}{{d|   {how}}}"))
            }
            Action::Ready(ready) => {
                self.each_pr(id, |p| p.draft = !ready);
                let now = if *ready { "ready for review" } else { "a draft again" };
                markup(&format!("{{g|✓}} {{f|#{n} is {now}}}"))
            }
            Action::Reopen => markup(&format!("{{g|✓}} {{f|Reopened #{n}}}")),
            Action::RequestChanges(_) => markup(&format!("{{g|✓}} {{f|Requested changes on #{n}}}")),
            Action::Review(who) => {
                let who: Vec<String> = who.iter().map(|w| format!("@{w}")).collect();
                markup(&format!("{{g|✓}} {{f|Asked for a review of #{n}}}{{d|   {}}}", esc(&who.join(" "))))
            }
            Action::Label { .. } => {
                markup(&format!("{{g|✓}} {{f|Labelled #{n}}}{{d|   {}}}", esc(note.unwrap_or_default())))
            }
            Action::Assign(who) => {
                let who: Vec<String> = who.iter().map(|w| format!("@{w}")).collect();
                markup(&format!("{{g|✓}} {{f|Assigned #{n}}}{{d|   {}}}", esc(&who.join(" "))))
            }
            Action::Title(title) => {
                self.each_pr(id, |p| p.title = title.clone());
                markup(&format!("{{g|✓}} {{f|Retitled #{n}}}"))
            }
            Action::Rerun | Action::Cancel => {
                let did = if run.job.action == Action::Rerun { "Re-ran" } else { "Cancelled" };
                // The pipeline screen stops asking once every run is done.
                self.poll.runs_asked = None;
                markup(&format!("{{g|✓}} {{f|{did} #{n}}}{{d|   {}}}", esc(note.unwrap_or_default())))
            }
            Action::Checkout { branch } => {
                let wher = note.map_or(String::new(), |n| format!(" {}", esc(n)));
                markup(&format!("{{g|✓}} {{f|Checked out #{n}}}{{d|   {}{wher}}}", esc(branch)))
            }
            Action::Open | Action::Refresh { .. } | Action::Create { .. } => return None,
        })
    }

    /// Reports a batch once its regular jobs are all answered. A merge of an
    /// already-green PR may still be running; it reports on its own.
    fn report_batch(&mut self, b: usize) {
        let bt = &self.batches[b];
        if bt.reported || bt.running > 0 || !bt.queue.is_empty() {
            return;
        }
        let (glyph, word) = match bt.action {
            Some(Action::Arm { .. }) => ("{a|◆}", "Armed"),
            Some(Action::Merge { .. }) => ("{g|✓}", "Merged"),
            Some(Action::Approve) => ("{g|✓}", "Approved"),
            Some(Action::Close) => ("{g|✓}", "Closed"),
            Some(Action::Update) => ("{g|✓}", "Updated"),
            Some(Action::Comment(_)) => ("{g|✓}", "Commented on"),
            _ => ("{g|✓}", "Done:"),
        };
        let count = bt.done + bt.merging_now.len();
        let mut notes: Vec<String> =
            bt.merging_now.iter().map(|n| format!("#{n} merging now (already green)")).collect();
        notes.extend(bt.failed.iter().map(|(n, r)| format!("#{n} failed: {r}")));
        notes.extend(bt.skipped.iter().map(|(n, r)| format!("skipped #{n}: {r}")));
        let tail = if notes.is_empty() { String::new() } else { format!("{{d|   {}}}", esc(&notes.join(" · "))) };
        let line = markup(&format!("{glyph} {{f|{word} {}}}{tail}", plural(count, "PR")));
        self.batches[b].reported = true;
        self.say(line, Mode::Normal);
    }

    pub fn prompt_line(&self, p: &Prompt) -> Line {
        let one = p.targets.len() == 1;
        let what = if one { format!("#{}", p.targets[0].number) } else { plural(p.targets.len(), "PR") };
        let pr = self.find(&p.targets[0]);
        let text = match &p.kind {
            PromptKind::Merge { method, allowed } => {
                let wher = match (one, pr) {
                    (true, Some(pr)) => format!(" {} → {}", esc(&pr.id.repo), esc(&pr.base)),
                    _ => String::new(),
                };
                format!(
                    "{{f|Merge }}{{B|{what}}}{{f|{wher} with }}{{yB|{}}}{{f|?}}   {{d|y/Enter yes{}   n/Esc cancel}}",
                    method.word(),
                    alts(*method, allowed, "merge commit")
                )
            }
            PromptKind::Arm { method, allowed } => format!(
                "{{f|Arm auto-merge on }}{{B|{what}}}{{f| with }}{{yB|{}}}{{f|?}}   {{d|y/Enter yes{}   n/Esc cancel}}",
                method.word(),
                alts(*method, allowed, "merge")
            ),
            PromptKind::ArmMethod { default, allowed } => {
                let opts: String = ORDER
                    .iter()
                    .filter(|m| allowed.contains(m))
                    .map(|m| {
                        if m == default {
                            format!("   {{aB|{} {}}}", key_of(*m), m.word())
                        } else {
                            format!("   {{f|{}}}{{d| {}}}", key_of(*m), m.word())
                        }
                    })
                    .collect();
                format!("{{f|Arm auto-merge on {what} with}}{opts}      {{d|Enter {}   Esc cancel}}", default.word())
            }
            PromptKind::ArmInstead => {
                let why = match pr.map(|p| verdict(p, &self.caps_of(p))) {
                    Some(Verdict::Waiting(w)) => not_ready(&w),
                    _ => "not ready".into(),
                };
                format!(
                    "{{f|{what} isn't ready: {}. Arm auto-merge instead?}}   {{d|y/A arm   n/Esc cancel}}",
                    esc(&why)
                )
            }
            PromptKind::MergeNow { method } => format!(
                "{{f|{what} can merge as it is, so there is nothing to arm. Merge with }}{{yB|{}}}{{f|?}}   {{d|y/Enter yes   n/Esc cancel}}",
                method.word()
            ),
            PromptKind::AllowAuto { method } => format!(
                "{{f|{} has auto-merge off. Turn it on and arm }}{{B|{what}}}{{f| with }}{{yB|{}}}{{f|?}}   {{d|y/Enter yes   n/Esc cancel}}",
                pr.map_or(String::new(), |p| esc(&p.id.repo)),
                method.word()
            ),
            PromptKind::Close => {
                format!("{{f|Close }}{{B|{what}}}{{f| without merging?}}   {{d|y/Enter yes   n/Esc cancel}}")
            }
            PromptKind::Approve => format!("{{f|Approve }}{{B|{what}}}{{f|?}}   {{d|y/Enter yes   n/Esc cancel}}"),
            PromptKind::Update => {
                format!("{{f|Bring }}{{B|{what}}}{{f| up to date with their base?}}   {{d|y/Enter yes   n/Esc cancel}}")
            }
            PromptKind::New(Action::Create { head, base, title, .. }) => format!(
                "{{f|Open a PR in {}: }}{{B|{}}}{{f| → {}, “{}”?}}   {{d|y/Enter yes   n/Esc cancel}}",
                esc(&p.targets[0].repo),
                esc(head),
                esc(base.as_deref().unwrap_or("its default branch")),
                esc(title)
            ),
            PromptKind::New(_) => String::new(),
            PromptKind::Several(a) => {
                format!("{{f|:{} on }}{{B|{what}}}{{f|?}}   {{d|y/Enter yes   n/Esc cancel}}", a.command())
            }
            PromptKind::Comment(_) => {
                format!("{{f|Comment on }}{{B|{what}}}{{f|?}}   {{d|y/Enter yes   n/Esc cancel}}")
            }
        };
        markup(&text)
    }

    /// `:new`, once the clone has been read: asks before anything is opened.
    /// The forge is the one a listed PR on that host has, else the host's
    /// name decides.
    pub fn request_new(&mut self, host: &str, repo: &str, action: Action) {
        let listed = self.sections.iter().flat_map(|s| &s.prs).find(|p| p.id.host == host).map(|p| p.id.forge);
        let gitlab = if host.contains("gitlab") { pullpen_core::Forge::GitLab } else { pullpen_core::Forge::GitHub };
        let id = PrId { forge: listed.unwrap_or(gitlab), host: host.into(), repo: repo.into(), number: 0 };
        self.ask(PromptKind::New(action), vec![id], Origin::Command, None);
    }

    /// The text back from $EDITOR for a bare `c`.
    pub fn comment_text(&mut self, ids: Vec<PrId>, text: String) -> Vec<Effect> {
        if text.trim().is_empty() {
            self.say(markup("{d|Comment empty; nothing posted}"), Mode::Normal);
            return vec![];
        }
        self.request_simple(ids, Action::Comment(text), Origin::Key, None)
    }

    /// The URL to open for each target.
    /// What `o` opens: the PR, or in Deploys and Merged its pipeline.
    pub fn urls(&self, ids: &[PrId]) -> Vec<String> {
        let landed = matches!(
            self.sections.get(self.active).map(|s| s.section.view),
            Some(pullpen_core::View::Merged | pullpen_core::View::Deploys)
        );
        let pipeline = landed && self.detail.is_none();
        ids.iter()
            .filter_map(|id| self.find(id))
            .map(|p| match &p.post_merge {
                Some(pl) if pipeline && !pl.url.is_empty() => pl.url.clone(),
                _ => p.url.clone(),
            })
            .collect()
    }

    pub fn refused(&mut self, what: &str) {
        self.say(vec![s(what, Tok::Dim)], Mode::Normal);
    }
}
