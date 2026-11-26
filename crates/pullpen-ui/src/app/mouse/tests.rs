use std::time::Instant;

use pullpen_fixtures as fx;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::*;
use crate::app::Input;
use crate::app::SectionState;
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

fn draw(a: &mut App, w: u16) -> Vec<String> {
    let area = Rect::new(0, 0, w, 40);
    let mut buf = Buffer::empty(area);
    a.render(area, &mut buf);
    (0..40).map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect()).collect()
}

/// The screen row showing `text`.
fn y_of(rows: &[String], text: &str) -> u16 {
    rows.iter().position(|r| r.contains(text)).unwrap_or_else(|| panic!("{text:?} is not on screen")) as u16
}

fn click(a: &mut App, x: u16, y: u16) {
    a.mouse(Mouse::Click { x, y }, Instant::now());
}

/// Two clicks 100ms apart, a second after any click before them.
fn twice(a: &mut App, x: u16, y: u16) {
    let t = Instant::now() + Duration::from_secs(1);
    a.mouse(Mouse::Click { x, y }, t);
    a.mouse(Mouse::Click { x, y }, t + Duration::from_millis(100));
}

fn wheel(a: &mut App, x: u16, down: bool) {
    a.mouse(Mouse::Scroll { x, down }, Instant::now());
}

fn selected(a: &App) -> u64 {
    a.section().selected().unwrap().id.number
}

/// #812 in the Open section, `Enter` then `w`: its pipeline over its detail
/// view, the runs answered.
fn in_pipeline() -> App {
    let mut a = app();
    a.active = 5;
    a.sections[5].sel = a.sections[5].prs.iter().position(|p| p.id.number == 812).unwrap();
    let id = a.section().prs[a.section().sel].id.clone();
    a.key(Key::Enter, Instant::now());
    a.key(Key::Char('w'), Instant::now());
    a.runs_loaded(id, fx::runs(812));
    a
}

/// `in_pipeline`, then Enter on `build`, its log answered.
fn in_job() -> App {
    let mut a = in_pipeline();
    let id = a.pipe.as_ref().unwrap().id.clone();
    a.key(Key::Enter, Instant::now());
    a.log_loaded(id, "79".into(), fx::LOG);
    a
}

/// #4821's detail view, its diff of fourteen files loaded.
fn in_detail() -> App {
    let mut a = app();
    let id = a.section().prs[0].id.clone();
    a.details.insert(id.clone(), fx::detail(4821));
    a.diffs.insert(id, fx::diff(4821));
    a.key(Key::Enter, Instant::now());
    a
}

fn job(a: &App) -> &crate::views::pipeline::JobView {
    a.pipe.as_ref().unwrap().job.as_ref().unwrap()
}

#[test]
fn clicking_a_tab_switches_and_a_row_selects_and_twice_opens() {
    let mut a = app();
    draw(&mut a, 120);
    let t = Instant::now();
    // "  Mine 6    Needs my review 4 …": column 12 is inside the second tab.
    a.mouse(Mouse::Click { x: 12, y: 0 }, t);
    assert_eq!(a.active, 1);
    a.active = 6;
    draw(&mut a, 120);
    a.mouse(Mouse::Click { x: 30, y: 5 }, t);
    assert_eq!((a.section().sel, a.detail.is_none()), (2, true));
    a.mouse(Mouse::Click { x: 30, y: 5 }, t + Duration::from_millis(100));
    assert!(a.detail.is_some(), "a double click opens the row");
}

#[test]
fn the_wheel_moves_the_selection() {
    let mut a = app();
    draw(&mut a, 120);
    a.mouse(Mouse::Scroll { x: 0, down: true }, Instant::now());
    assert_eq!(a.section().sel, 3);
}

#[test]
fn a_click_in_a_grouped_view_selects_the_row_under_it_not_a_heading() {
    let mut a = app();
    // Armed, Deploys, Merged.
    for view in [2, 3, 4] {
        a.active = view;
        let want = a.section().visible().last().unwrap().id.number;
        let rows = draw(&mut a, 120);
        click(&mut a, 30, y_of(&rows[1..], &format!("#{want} ")) + 1);
        assert_eq!(selected(&a), want, "section {view}");
        // Row 2 is the first group's heading.
        click(&mut a, 30, 2);
        assert_eq!(selected(&a), want, "a heading is not a row, section {view}");
    }
}

#[test]
fn a_double_click_in_deploys_opens_the_pipeline() {
    let mut a = app();
    a.active = 3;
    let rows = draw(&mut a, 120);
    twice(&mut a, 30, y_of(&rows, "#812 "));
    assert_eq!(a.pipe.as_ref().map(|p| p.id.number), Some(812));
    assert!(a.detail.is_none(), "as Enter does there, not the detail view");
}

#[test]
fn a_click_in_the_preview_pane_selects_nothing() {
    let mut a = app();
    let rows = draw(&mut a, 120);
    let pane = rows[5].chars().position(|c| c == '│').unwrap() as u16;
    click(&mut a, pane + 5, 5);
    assert_eq!(a.section().sel, 0);
    click(&mut a, pane - 5, 5);
    assert_eq!(a.section().sel, 2, "the same row, left of the preview");
}

#[test]
fn the_pipeline_screen_takes_the_click_not_the_list_beneath_it() {
    let mut a = in_pipeline();
    a.key(Key::Char('q'), Instant::now());
    a.key(Key::Char('q'), Instant::now());
    a.key(Key::Char('w'), Instant::now());
    assert!(a.pipe.is_some() && a.detail.is_none(), "the pipeline, straight over the list");
    let rows = draw(&mut a, 120);
    click(&mut a, 30, y_of(&rows, "lint"));
    assert_eq!((selected(&a), a.pipe.as_ref().unwrap().sel), (812, 0));
    wheel(&mut a, 30, true);
    assert_eq!((selected(&a), a.pipe.as_ref().unwrap().sel), (812, 3), "and the wheel");
}

#[test]
fn a_job_row_selects_and_twice_opens_the_job() {
    let mut a = in_pipeline();
    let rows = draw(&mut a, 120);
    for not_a_job in ["after merge", "zo to show"] {
        twice(&mut a, 30, y_of(&rows, not_a_job));
        assert_eq!(a.pipe.as_ref().unwrap().sel, 2, "{not_a_job}");
    }
    click(&mut a, 30, y_of(&rows, "go mod verify"));
    assert_eq!(a.pipe.as_ref().unwrap().sel, 1);
    assert!(a.pipe.as_ref().unwrap().job.is_none());
    twice(&mut a, 30, y_of(&rows, "lint"));
    assert_eq!(job(&a).name, "lint");
}

#[test]
fn the_click_that_opens_a_screen_does_not_count_towards_opening_a_row_of_it() {
    let mut a = app();
    a.active = 3;
    let rows = draw(&mut a, 120);
    let y = y_of(&rows, "#812 ");
    let t = Instant::now();
    for ms in [0, 100, 200] {
        a.mouse(Mouse::Click { x: 30, y }, t + Duration::from_millis(ms));
        draw(&mut a, 120);
    }
    let p = a.pipe.as_ref().unwrap();
    assert!(p.job.is_none(), "three quick clicks open the pipeline and stop there");
}

#[test]
fn clicking_a_pane_focuses_it() {
    let mut a = in_detail();
    draw(&mut a, 120);
    click(&mut a, 80, 20);
    assert_eq!(a.detail.as_ref().unwrap().focus, Pane::Diff);
    click(&mut a, 10, 20);
    assert_eq!(a.detail.as_ref().unwrap().focus, Pane::Overview);
    click(&mut a, 10, 3);
    assert!(a.detail.is_some(), "the rows above the panes do nothing");

    let mut a = in_job();
    let rows = draw(&mut a, 120);
    click(&mut a, 10, y_of(&rows, "Getting source from Git"));
    assert_eq!((job(&a).steps, job(&a).step), (true, 2));
    click(&mut a, 80, 20);
    assert!(!job(&a).steps, "the log has the focus");
    twice(&mut a, 10, y_of(&rows, "Getting source from Git"));
    assert_eq!((job(&a).steps, job(&a).follow), (false, false), "twice on a step jumps to it, as Enter does");
}

#[test]
fn the_wheel_scrolls_the_pane_under_it_and_the_log_stops_following() {
    let mut a = in_job();
    draw(&mut a, 120);
    assert!(job(&a).follow && !job(&a).steps);
    wheel(&mut a, 10, true);
    assert_eq!((job(&a).step, job(&a).steps, job(&a).follow), (3, false, true), "over the steps: the step moves");
    wheel(&mut a, 80, false);
    assert_eq!((job(&a).steps, job(&a).follow), (false, false), "over the log: it scrolls up and lets go");
}

#[test]
fn the_wheel_stops_at_the_end_of_a_pane() {
    let mut a = in_detail();
    draw(&mut a, 120);
    for _ in 0..100 {
        wheel(&mut a, 80, true);
    }
    let d = a.detail.clone().unwrap();
    assert_eq!(d.focus, Pane::Overview, "the wheel leaves the focus alone");
    assert_eq!(d.scroll[0], 0);
    a.key(Key::Char('l'), Instant::now());
    a.key(Key::Char('G'), Instant::now());
    assert_eq!(d.scroll[1], a.detail.as_ref().unwrap().scroll[1], "where G stops");
    assert!(d.scroll[1] > 0);
}

#[test]
fn the_breadcrumb_goes_back_one_level() {
    let mut a = in_job();
    draw(&mut a, 120);
    click(&mut a, 5, 0);
    assert!(a.pipe.as_ref().is_some_and(|p| p.job.is_none()), "job to pipeline");
    click(&mut a, 5, 0);
    assert!(a.pipe.is_none() && a.detail.is_some(), "pipeline to the detail view it was opened from");
    click(&mut a, 5, 0);
    assert!(a.detail.is_none(), "detail view to the list");
    assert_eq!(a.active, 5, "and there row 0 is the tabs again; column 5 is the first");
    draw(&mut a, 120);
    click(&mut a, 12, 0);
    assert_eq!(a.active, 1);
}

#[test]
fn a_picker_row_selects_and_twice_opens_the_file() {
    let mut a = in_detail();
    a.key(Key::Char('F'), Instant::now());
    let rows = draw(&mut a, 120);
    let y = y_of(&rows, "part5.ts");
    click(&mut a, 40, y);
    assert_eq!(a.detail.as_ref().unwrap().picker.as_ref().map(|p| p.1), Some(4));
    click(&mut a, 5, 0);
    assert!(a.detail.as_ref().is_some_and(|d| d.picker.is_some()), "a click off the rows leaves the list open");
    twice(&mut a, 40, y);
    let d = a.detail.as_ref().unwrap();
    assert_eq!((d.file, d.picker.is_none(), d.focus), (4, true, Pane::Diff));
}

#[test]
fn a_click_at_a_confirm_prompt_changes_nothing() {
    let mut a = app();
    a.key(Key::Char('x'), Instant::now());
    assert!(matches!(a.input, Input::Confirm(_)), "x asks before closing");
    draw(&mut a, 120);
    twice(&mut a, 30, 5);
    click(&mut a, 12, 0);
    wheel(&mut a, 30, true);
    assert!(matches!(a.input, Input::Confirm(_)), "still asking");
    assert_eq!((a.active, a.section().sel, a.detail.is_none()), (6, 0, true));
}
