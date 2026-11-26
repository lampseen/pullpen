use std::time::{Duration, Instant};

use pullpen_fixtures as fx;

use super::*;
use crate::app::{DefaultMethod, SectionState};
use crate::glyphs::Glyphs;
use crate::paint::Theme;
use crate::palette::{Palette, Tier};

fn app() -> App {
    // The agent-opened rows, as a section of their own (not a default).
    let sections = fx::sections()
        .into_iter()
        .chain([fx::agent_section()])
        .map(|s| SectionState::new(s.section, s.prs, s.total))
        .collect();
    let mut a = App::new(sections, Theme { palette: Palette::new(Tier::True), glyphs: Glyphs::Unicode }, fx::NOW);
    a.active = 6;
    a
}

fn press(a: &mut App, keys: &[Key]) -> Vec<Effect> {
    keys.iter().flat_map(|k| a.key(*k, Instant::now())).collect()
}

fn text(a: &mut App, t: &str) {
    for c in t.chars() {
        a.key(Key::Char(c), Instant::now());
    }
}

#[test]
fn gg_goes_to_the_top_and_a_lone_g_expires() {
    let mut a = app();
    press(&mut a, &[Key::Char('G')]);
    assert_eq!(a.section().sel, 22);
    press(&mut a, &[Key::Char('g'), Key::Char('g')]);
    assert_eq!(a.section().sel, 0);

    let t0 = Instant::now();
    a.key(Key::Char('j'), t0);
    a.key(Key::Char('g'), t0);
    a.key(Key::Char('g'), t0 + PENDING + Duration::from_millis(1));
    assert_eq!(a.section().sel, 1, "the second g came too late to make gg");
}

#[test]
fn any_other_key_at_a_confirm_cancels_and_is_discarded() {
    let mut a = app();
    press(&mut a, &[Key::Char('x')]);
    assert!(matches!(a.input, Input::Confirm(_)));
    let fx = press(&mut a, &[Key::Char('j')]);
    assert!(fx.is_empty());
    assert!(matches!(a.input, Input::Normal));
    assert_eq!(a.section().sel, 0, "the j was discarded, not acted on");
}

#[test]
fn filter_fades_then_enter_hides_then_esc_clears() {
    let mut a = app();
    press(&mut a, &[Key::Char('/')]);
    text(&mut a, "ledger");
    assert_eq!(a.section().visible().len(), 23, "typing only fades non-matches");
    assert_eq!(a.section().selected().unwrap().id.number, 4821);
    press(&mut a, &[Key::Enter]);
    assert_eq!(a.section().visible().len(), 3);
    press(&mut a, &[Key::Char('n')]);
    assert_eq!(a.section().selected().unwrap().id.number, 1199);
    press(&mut a, &[Key::Esc]);
    assert_eq!(a.section().visible().len(), 23);
}

#[test]
fn space_marks_and_moves_on_and_v_marks_the_range() {
    let mut a = app();
    press(&mut a, &[Key::Char(' ')]);
    assert_eq!((a.section().marks.len(), a.section().sel), (1, 1));
    press(&mut a, &[Key::Char('j'), Key::Char('j'), Key::Char('V')]);
    assert_eq!(a.section().marks.len(), 4, "rows 0 to 3");
    press(&mut a, &[Key::Esc]);
    assert!(a.section().marks.is_empty());
}

#[test]
fn tab_fixes_a_typo() {
    let mut a = app();
    press(&mut a, &[Key::Char(':')]);
    text(&mut a, "mrege #812");
    press(&mut a, &[Key::Enter, Key::Tab]);
    match &a.input {
        Input::Command(e) => assert_eq!(e.text, "merge #812"),
        other => panic!("expected the command line back, got {other:?}"),
    }
}

#[test]
fn section_command_takes_short_names() {
    let mut a = app();
    press(&mut a, &[Key::Char(':')]);
    text(&mut a, "section armed");
    press(&mut a, &[Key::Enter]);
    assert_eq!(a.active, 2);
}

#[test]
fn a_batch_skips_the_ineligible_by_name() {
    let mut a = app();
    for i in [9, 11] {
        a.section_mut().sel = i;
        press(&mut a, &[Key::Char(' ')]);
    }
    press(&mut a, &[Key::Char('M'), Key::Char('y')]);
    let line: String = a.message.as_ref().unwrap().line.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(line, "✓ Merged 0 PRs   skipped #1203: draft · skipped #143: test failing");
}

#[test]
fn repeated_commands_enter_history_once() {
    let mut a = app();
    for _ in 0..2 {
        press(&mut a, &[Key::Char(':')]);
        text(&mut a, "refresh");
        press(&mut a, &[Key::Enter]);
    }
    assert_eq!(a.history, vec!["refresh".to_string()]);
}

#[test]
fn ctrl_r_hash_inserts_the_selected_ref() {
    let mut a = app();
    press(&mut a, &[Key::Char('j'), Key::Char(':')]);
    text(&mut a, "approve ");
    press(&mut a, &[Key::Ctrl('r'), Key::Char('#')]);
    match &a.input {
        Input::Command(e) => assert_eq!(e.text, "approve #812"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_prompt_about_a_pr_that_merged_meanwhile_goes_away() {
    let mut a = app();
    a.section_mut().sel = 1;
    let fx = press(&mut a, &[Key::Char('M'), Key::Char('y'), Key::Char('x')]);
    assert!(matches!(a.input, Input::Confirm(_)), "close #812 asked while its merge runs");
    for e in fx {
        if let Effect::Run(job) = e {
            a.job_done(pullpen_core::JobResult { job, outcome: pullpen_core::Outcome::Done, note: None });
        }
    }
    assert!(matches!(a.input, Input::Normal));
}

#[test]
fn u_updates_the_branch_and_says_how() {
    let mut a = app();
    a.section_mut().sel = 1;
    let fx = press(&mut a, &[Key::Char('u')]);
    let job = fx.iter().find_map(|e| if let Effect::Run(j) = e { Some(j.clone()) } else { None }).unwrap();
    assert_eq!(job.action, pullpen_core::Action::Update);
    a.job_done(pullpen_core::JobResult { job, outcome: pullpen_core::Outcome::Done, note: None });
    let line: String = a.message.as_ref().unwrap().line.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(line, "✓ Updated #812   rebased onto main", "#812 is on GitLab, which rebases");
}

#[test]
fn m_on_a_pr_that_is_only_behind_names_the_key_that_updates_it() {
    let mut a = app();
    let id = a.section().prs[1].id.clone();
    a.caps.insert(
        format!("{}/{}", id.host, id.repo),
        pullpen_core::Capabilities { requires_up_to_date: true, ..Default::default() },
    );
    a.section_mut().prs[1].mergeable = pullpen_core::Mergeable::Behind;
    a.section_mut().sel = 1;
    press(&mut a, &[Key::Char('M')]);
    let line: String = a.message.as_ref().unwrap().line.iter().map(|s| s.text.as_str()).collect();
    assert!(line.starts_with("E  #812 is behind main") && line.ends_with("u updates the branch"), "{line}");
    assert!(matches!(a.input, Input::Normal), "no arm-instead prompt: arming would wait forever");
}

/// The first row that `A` would arm rather than merge or disarm.
fn unarmed_waiting(a: &App) -> usize {
    let s = a.section();
    (0..s.prs.len())
        .find(|&i| s.prs[i].auto_merge.is_none() && a.readiness(&s.prs[i]) == pullpen_core::Readiness::Waiting)
        .unwrap()
}

fn refuse(a: &mut App, fx: Vec<Effect>) {
    for e in fx {
        if let Effect::Run(job) = e {
            let outcome = pullpen_core::Outcome::Failed { reason: "Auto merge is not allowed".into() };
            a.job_done(pullpen_core::JobResult { job, outcome, note: None });
        }
    }
}

#[test]
fn a_refused_arm_takes_the_diamond_back_off_so_the_next_a_arms_again() {
    let mut a = app();
    a.ui.default_method = DefaultMethod::Fixed(pullpen_core::MergeMethod::Squash);
    let i = unarmed_waiting(&a);
    a.section_mut().sel = i;
    let fx = press(&mut a, &[Key::Char('A')]);
    assert!(a.section().prs[i].auto_merge.is_some(), "armed at once, before the forge answers");
    refuse(&mut a, fx);
    assert!(a.section().prs[i].auto_merge.is_none(), "the forge refused: the row is not armed");

    let fx = press(&mut a, &[Key::Char('A')]);
    let again = fx.iter().find_map(|e| if let Effect::Run(j) = e { Some(j.action.clone()) } else { None });
    assert!(
        matches!(again, Some(pullpen_core::Action::Arm { .. })),
        "not a disarm of an arm that never took: {again:?}"
    );
}

#[test]
fn a_refused_arm_waiting_its_turn_in_a_batch_is_taken_back_off_too() {
    let mut a = app();
    a.ui.default_method = DefaultMethod::Fixed(pullpen_core::MergeMethod::Squash);
    let rows: Vec<usize> = {
        let s = a.section();
        (0..s.prs.len())
            .filter(|&i| s.prs[i].auto_merge.is_none() && a.readiness(&s.prs[i]) == pullpen_core::Readiness::Waiting)
            .take(2)
            .collect()
    };
    assert_eq!(rows.len(), 2, "the fixture has two rows to arm");
    let ids: Vec<_> = rows.iter().map(|&i| a.section().prs[i].id.clone()).collect();
    a.section_mut().marks = ids;
    let mut fx = press(&mut a, &[Key::Char('A'), Key::Char('y')]);
    while !fx.is_empty() {
        let mut next = Vec::new();
        for e in fx {
            if let Effect::Run(job) = e {
                let outcome = pullpen_core::Outcome::Failed { reason: "no".into() };
                next.extend(a.job_done(pullpen_core::JobResult { job, outcome, note: None }));
            }
        }
        fx = next;
    }
    for i in rows {
        assert!(a.section().prs[i].auto_merge.is_none(), "row {i} was refused");
    }
}

fn auto_merge_off(a: &mut App, i: usize, can_admin: bool) {
    let id = a.section().prs[i].id.clone();
    let caps = pullpen_core::Capabilities { auto_merge_off: true, can_admin, ..Default::default() };
    a.caps.insert(format!("{}/{}", id.host, id.repo), caps);
}

#[test]
fn a_in_a_repo_with_auto_merge_off_offers_its_admin_the_setting() {
    let mut a = app();
    let i = unarmed_waiting(&a);
    auto_merge_off(&mut a, i, true);
    a.section_mut().sel = i;
    let fx = press(&mut a, &[Key::Char('A')]);
    assert!(fx.is_empty(), "nothing is sent before the answer");
    assert!(matches!(&a.input, Input::Confirm(p) if matches!(p.kind, crate::app::act::PromptKind::AllowAuto { .. })));
    let fx = press(&mut a, &[Key::Char('y')]);
    let sent = fx.iter().find_map(|e| if let Effect::Run(j) = e { Some(j.action.clone()) } else { None });
    assert!(matches!(sent, Some(pullpen_core::Action::AllowAuto { .. })), "{sent:?}");
}

#[test]
fn a_in_a_repo_with_auto_merge_off_tells_anyone_else_and_sends_nothing() {
    let mut a = app();
    let i = unarmed_waiting(&a);
    auto_merge_off(&mut a, i, false);
    a.section_mut().sel = i;
    let fx = press(&mut a, &[Key::Char('A')]);
    assert!(fx.is_empty() && matches!(a.input, Input::Normal));
    let line: String = a.message.as_ref().unwrap().line.iter().map(|s| s.text.as_str()).collect();
    assert!(line.contains("has auto-merge off"), "{line}");
    assert!(a.section().prs[i].auto_merge.is_none());
}

#[test]
fn a_batch_arm_skips_a_repo_with_auto_merge_off_by_name() {
    let mut a = app();
    let i = unarmed_waiting(&a);
    auto_merge_off(&mut a, i, true);
    let n = a.section().prs[i].id.number;
    let other = (0..a.section().prs.len()).find(|&j| j != i).unwrap();
    a.section_mut().marks = vec![a.section().prs[i].id.clone(), a.section().prs[other].id.clone()];
    let fx = press(&mut a, &[Key::Char('A'), Key::Char('y')]);
    let sent = fx.iter().any(|e| matches!(e, Effect::Run(j) if j.target.number == n));
    assert!(!sent, "a batch never changes a repo's settings");
    let line: String = a.message.as_ref().unwrap().line.iter().map(|s| s.text.as_str()).collect();
    assert!(line.contains(&format!("skipped #{n}: repo has auto-merge off")), "{line}");
}

#[test]
fn a_on_a_pr_the_forge_would_merge_at_once_asks_to_merge_and_never_arms() {
    let mut a = app();
    a.ui.default_method = DefaultMethod::Fixed(pullpen_core::MergeMethod::Squash);
    let i = unarmed_waiting(&a);
    a.section_mut().prs[i].mergeable_now = true;
    a.section_mut().sel = i;
    let fx = press(&mut a, &[Key::Char('A')]);
    assert!(fx.is_empty(), "a row that is not green merges only on a yes");
    let fx = press(&mut a, &[Key::Char('y')]);
    let sent = fx.iter().find_map(|e| if let Effect::Run(j) = e { Some(j.action.clone()) } else { None });
    assert_eq!(sent, Some(pullpen_core::Action::Merge { method: pullpen_core::MergeMethod::Squash }));
}

#[test]
fn a_batch_arm_skips_a_pr_the_forge_would_merge_at_once() {
    let mut a = app();
    let i = unarmed_waiting(&a);
    a.section_mut().prs[i].mergeable_now = true;
    let n = a.section().prs[i].id.number;
    let other = (0..a.section().prs.len()).find(|&j| j != i).unwrap();
    a.section_mut().marks = vec![a.section().prs[i].id.clone(), a.section().prs[other].id.clone()];
    let fx = press(&mut a, &[Key::Char('A'), Key::Char('y')]);
    assert!(!fx.iter().any(|e| matches!(e, Effect::Run(j) if j.target.number == n)));
    let line: String = a.message.as_ref().unwrap().line.iter().map(|s| s.text.as_str()).collect();
    assert!(line.contains(&format!("skipped #{n}: can merge now")), "{line}");
}

fn said(a: &App) -> String {
    a.message.as_ref().unwrap().line.iter().map(|s| s.text.as_str()).collect()
}

fn sent(fx: &[Effect]) -> Option<pullpen_core::Action> {
    fx.iter().find_map(|e| if let Effect::Run(j) = e { Some(j.action.clone()) } else { None })
}

#[test]
fn a_configured_method_the_repo_does_not_allow_gives_way_to_the_repos_own() {
    use pullpen_core::MergeMethod::{Merge, Rebase, Squash};
    let mut a = app();
    a.ui.default_method = DefaultMethod::Fixed(Rebase);
    let i = unarmed_waiting(&a);
    a.section_mut().prs[i].repo_allowed_methods = vec![Squash, Merge];
    a.section_mut().prs[i].repo_default_method = Squash;
    a.section_mut().sel = i;
    let fx = press(&mut a, &[Key::Char('A')]);
    assert_eq!(sent(&fx), Some(pullpen_core::Action::Arm { method: Squash }));
}

#[test]
fn a_typed_method_the_repo_does_not_allow_is_refused_before_it_is_sent() {
    use pullpen_core::MergeMethod::{Merge, Squash};
    let mut a = app();
    let i = unarmed_waiting(&a);
    a.section_mut().prs[i].repo_allowed_methods = vec![Squash, Merge];
    a.section_mut().sel = i;
    let ids = a.targets(None);
    let fx = a.request_arm(ids.clone(), Some(pullpen_core::MergeMethod::Rebase), Origin::Key, None);
    assert!(fx.is_empty());
    assert!(said(&a).contains("doesn't allow rebase") && said(&a).ends_with("it allows squash, merge"), "{}", said(&a));
    a.request_merge(ids, Some(pullpen_core::MergeMethod::Rebase), Origin::Key, None);
    assert!(matches!(a.input, Input::Normal), "no merge prompt for a method that cannot work");
}

#[test]
fn a_on_a_pr_that_is_only_behind_names_the_key_that_updates_it() {
    let mut a = app();
    let id = a.section().prs[1].id.clone();
    a.caps.insert(
        format!("{}/{}", id.host, id.repo),
        pullpen_core::Capabilities { requires_up_to_date: true, ..Default::default() },
    );
    a.section_mut().prs[1].mergeable = pullpen_core::Mergeable::Behind;
    a.section_mut().sel = 1;
    let fx = press(&mut a, &[Key::Char('A')]);
    assert!(fx.is_empty(), "an arm would wait on an update nobody asked for");
    assert!(
        said(&a).starts_with("E  #812 is behind main") && said(&a).ends_with("u updates the branch"),
        "{}",
        said(&a)
    );
    assert!(a.section().prs[1].auto_merge.is_none());
}

fn done(a: &mut App, fx: Vec<Effect>) {
    for e in fx {
        if let Effect::Run(job) = e {
            a.job_done(pullpen_core::JobResult { job, outcome: pullpen_core::Outcome::Done, note: None });
        }
    }
}

#[test]
fn a_second_u_while_the_first_is_under_way_sends_nothing() {
    let mut a = app();
    a.section_mut().sel = 1;
    let fx = press(&mut a, &[Key::Char('u')]);
    assert!(press(&mut a, &[Key::Char('u')]).is_empty(), "the forge has not answered the first");
    assert_eq!(said(&a), "#812 is already being updated");
    done(&mut a, fx);
    assert!(press(&mut a, &[Key::Char('u')]).is_empty(), "the forge answered, and is still rewriting the branch");
}

#[test]
fn an_updated_branch_is_not_believed_behind_until_the_forge_catches_up() {
    use pullpen_core::Mergeable::{Behind, Clean, Unknown};
    let mut a = app();
    a.section_mut().sel = 1;
    let i = a.active;
    let id = a.section().prs[1].id.clone();
    let fx = press(&mut a, &[Key::Char('u')]);
    done(&mut a, fx);
    let read = |a: &App, m| {
        let mut prs = a.section().prs.clone();
        prs[1].mergeable = m;
        (prs, a.section().total)
    };
    let (prs, total) = read(&a, Behind);
    a.synced(i, prs, total);
    assert_eq!(a.find(&id).unwrap().mergeable, Unknown, "a read that came too early");
    let (prs, total) = read(&a, Clean);
    a.synced(i, prs, total);
    assert_eq!(a.find(&id).unwrap().mergeable, Clean);
    let (prs, total) = read(&a, Behind);
    a.synced(i, prs, total);
    assert_eq!(a.find(&id).unwrap().mergeable, Behind, "behind again after the update landed is news");
}

#[test]
fn an_update_with_nothing_to_bring_in_is_an_answer_not_an_error() {
    let mut a = app();
    a.section_mut().sel = 1;
    for e in press(&mut a, &[Key::Char('u')]) {
        if let Effect::Run(job) = e {
            let reason = "There are no new commits on the base branch.".to_string();
            a.job_done(pullpen_core::JobResult { job, outcome: pullpen_core::Outcome::Failed { reason }, note: None });
        }
    }
    assert_eq!(said(&a), "#812 is already up to date with its base");
}

#[test]
fn u_on_a_merged_pr_is_refused_here() {
    let mut a = app();
    let at = a.now;
    a.section_mut().prs[1].state = pullpen_core::PrState::Merged { at, method: pullpen_core::MergeMethod::Squash };
    a.section_mut().sel = 1;
    assert!(press(&mut a, &[Key::Char('u')]).is_empty());
    assert_eq!(said(&a), "E  #812 is already merged");
}

#[test]
fn an_action_that_finishes_after_its_row_left_still_says_so() {
    let mut a = app();
    a.section_mut().sel = 1;
    let fx = press(&mut a, &[Key::Char('a')]);
    for s in &mut a.sections {
        s.prs.retain(|p| p.id.number != 812);
    }
    done(&mut a, fx);
    assert_eq!(said(&a), "✓ :approve #812 done   it has left this list");
}

fn command(a: &mut App, line: &str) -> Vec<Effect> {
    a.key(Key::Char(':'), Instant::now());
    text(a, line);
    a.key(Key::Enter, Instant::now())
}

#[test]
fn rerun_and_cancel_are_commands_and_say_which_jobs() {
    let mut a = app();
    a.section_mut().sel = 1;
    let fx = command(&mut a, "rerun");
    assert_eq!(sent(&fx), Some(pullpen_core::Action::Rerun));
    for e in fx {
        if let Effect::Run(job) = e {
            let note = Some("3 jobs".to_string());
            a.job_done(pullpen_core::JobResult { job, outcome: pullpen_core::Outcome::Done, note });
        }
    }
    assert_eq!(said(&a), "✓ Re-ran #812   3 jobs");
    assert_eq!(sent(&command(&mut a, "cancel #812")), Some(pullpen_core::Action::Cancel));
}

#[test]
fn a_command_on_several_prs_asks_once() {
    let mut a = app();
    a.section_mut().marks = a.section().prs[..2].iter().map(|p| p.id.clone()).collect();
    assert_eq!(sent(&command(&mut a, "rerun")), None, "nothing runs before the answer");
    let Input::Confirm(p) = &a.input else { panic!("no prompt") };
    let line: String = a.prompt_line(p).iter().map(|s| s.text.as_str()).collect();
    assert!(line.starts_with(":rerun on 2 PRs?"), "{line}");
    let fx = press(&mut a, &[Key::Char('y')]);
    assert_eq!(sent(&fx), Some(pullpen_core::Action::Rerun));
}

#[test]
fn review_workflow_commands_carry_what_was_typed() {
    use pullpen_core::Action;
    let mut a = app();
    a.section_mut().sel = 1;
    assert_eq!(sent(&command(&mut a, "draft")), Some(Action::Ready(false)));
    assert_eq!(sent(&command(&mut a, "reopen #812")), Some(Action::Reopen));
    assert_eq!(
        sent(&command(&mut a, "request-changes rename the flag")),
        Some(Action::RequestChanges("rename the flag".into()))
    );
    assert_eq!(
        sent(&command(&mut a, "review #812 @mara jkoh")),
        Some(Action::Review(vec!["mara".into(), "jkoh".into()]))
    );
    assert_eq!(sent(&command(&mut a, "review")), None);
    assert_eq!(said(&a), "E  review: say who should review, as @login");
}

#[test]
fn marking_ready_shows_on_the_row_once_the_forge_agrees() {
    let mut a = app();
    a.section_mut().sel = 1;
    a.section_mut().prs[1].draft = true;
    let fx = command(&mut a, "ready");
    assert!(a.section().prs[1].draft, "not before the forge answers");
    done(&mut a, fx);
    assert!(!a.section().prs[1].draft);
    assert_eq!(said(&a), "✓ #812 is ready for review");
}

#[test]
fn label_assign_and_title_commands_carry_what_was_typed() {
    use pullpen_core::Action;
    let mut a = app();
    a.me = "ash".into();
    a.section_mut().sel = 1;
    let fx = command(&mut a, "label +infra ci -wip");
    assert_eq!(sent(&fx), Some(Action::Label { add: vec!["infra".into(), "ci".into()], remove: vec!["wip".into()] }));
    for e in fx {
        if let Effect::Run(job) = e {
            let note = Some("now ci, infra".to_string());
            a.job_done(pullpen_core::JobResult { job, outcome: pullpen_core::Outcome::Done, note });
        }
    }
    assert_eq!(said(&a), "✓ Labelled #812   now ci, infra");
    assert_eq!(sent(&command(&mut a, "assign")), Some(Action::Assign(vec!["ash".into()])), "bare :assign is yourself");
    assert_eq!(sent(&command(&mut a, "assign #812 @mara")), Some(Action::Assign(vec!["mara".into()])));
    let fx = command(&mut a, "title Runner: retry on 502");
    assert_eq!(sent(&fx), Some(Action::Title("Runner: retry on 502".into())));
    done(&mut a, fx);
    assert_eq!(a.section().prs[1].title, "Runner: retry on 502");
    assert_eq!(sent(&command(&mut a, "label")), None);
    assert_eq!(said(&a), "E  label: say which, as +add or -remove");
}

#[test]
fn a_title_is_for_one_pr() {
    let mut a = app();
    a.section_mut().marks = a.section().prs[..2].iter().map(|p| p.id.clone()).collect();
    assert_eq!(sent(&command(&mut a, "title Same for both")), None);
    assert_eq!(said(&a), "E  title: one PR at a time");
}

fn new_pr() -> pullpen_core::Action {
    pullpen_core::Action::Create {
        head: "feat/retry".into(),
        base: None,
        title: "Retry on 502".into(),
        body: String::new(),
    }
}

#[test]
fn new_asks_what_it_will_open_then_says_where_it_is() {
    let mut a = app();
    let fx = command(&mut a, "new release");
    assert!(fx.contains(&Effect::New { base: Some("release".into()) }), "the clone is read off the UI's hands");
    a.request_new("gitlab.com", "platform/runner", new_pr());
    let Input::Confirm(p) = &a.input else { panic!("no prompt") };
    assert_eq!(p.targets[0].forge, pullpen_core::Forge::GitLab, "the forge a listed PR on that host has");
    let line: String = a.prompt_line(p).iter().map(|s| s.text.as_str()).collect();
    assert!(
        line.starts_with("Open a PR in platform/runner: feat/retry → its default branch, “Retry on 502”?"),
        "{line}"
    );
    let fx = press(&mut a, &[Key::Char('y')]);
    assert_eq!(sent(&fx), Some(new_pr()));
    for e in fx {
        if let Effect::Run(job) = e {
            let note = Some("https://gitlab.com/platform/runner/-/merge_requests/813".to_string());
            a.job_done(pullpen_core::JobResult { job, outcome: pullpen_core::Outcome::Done, note });
        }
    }
    assert_eq!(said(&a), "✓ Opened https://gitlab.com/platform/runner/-/merge_requests/813");
}

#[test]
fn a_refused_new_pr_names_the_repo_not_a_number() {
    let mut a = app();
    a.request_new("github.example.com", "acme/api", new_pr());
    let Input::Confirm(p) = &a.input else { panic!("no prompt") };
    assert_eq!(p.targets[0].forge, pullpen_core::Forge::GitHub);
    let fx = press(&mut a, &[Key::Enter]);
    for e in fx {
        if let Effect::Run(job) = e {
            let outcome = pullpen_core::Outcome::Failed { reason: "No commits between main and feat/retry".into() };
            a.job_done(pullpen_core::JobResult { job, outcome, note: None });
        }
    }
    assert!(said(&a).contains(":new acme/api: No commits between main and feat/retry"), "{}", said(&a));
}

#[test]
fn mouse_turns_the_mouse_off_and_the_next_one_turns_it_back_on() {
    let mut a = app();
    assert!(a.ui.mouse, "on unless asked");
    command(&mut a, "mouse");
    assert_eq!((a.ui.mouse, said(&a).as_str()), (false, "mouse off: the terminal selects text again"));
    command(&mut a, "mouse");
    assert_eq!((a.ui.mouse, said(&a).as_str()), (true, "mouse on"));
}
