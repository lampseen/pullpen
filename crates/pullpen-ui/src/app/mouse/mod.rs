//! The mouse, additive only (design §13): nothing here that a key cannot do.
//! A click finds what the last frame drew under it, then presses that key.

use std::time::{Duration, Instant};

use pullpen_core::View;

use super::{App, Effect, Input};
use crate::keymap::Key;
use crate::views::detail::{self, Pane};
use crate::views::list::PREVIEW_W;
use crate::views::pipeline::{self, JOB_TOP, steps};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouse {
    Click { x: u16, y: u16 },
    Scroll { x: u16, down: bool },
}

/// Two clicks on one row this close together open it.
const DOUBLE: Duration = Duration::from_millis(400);
/// Rows one notch of the wheel moves.
const WHEEL: usize = 3;

impl App {
    /// Records which item each screen row from `top` down shows.
    pub(super) fn set_hits(&self, top: usize, rows: impl IntoIterator<Item = Option<usize>>) {
        let mut h = self.hits.borrow_mut();
        *h = vec![None; top];
        h.extend(rows);
    }

    pub fn mouse(&mut self, m: Mouse, now: Instant) -> Vec<Effect> {
        // A prompt or a line being typed is answered by keys alone, so a
        // stray click can neither confirm nor cancel it.
        if !matches!(self.input, Input::Normal) {
            return vec![];
        }
        self.pending = None;
        match m {
            Mouse::Click { x, y } => self.click(usize::from(x), usize::from(y), now),
            Mouse::Scroll { x, down } => self.wheel(usize::from(x), down, now),
        }
    }

    fn click(&mut self, x: usize, y: usize, now: Instant) -> Vec<Effect> {
        let hit = self.hits.borrow().get(y).copied().flatten();
        let h = self.height;
        // Inside the two panes or on their headings, which sit a row above.
        let panes = |top: usize| (top - 1..h.saturating_sub(2)).contains(&y);
        let n_steps = self.open_job().map_or(0, |(_, j, _)| steps(j, self.current_log(), self.now).len());
        // The key the click stands for, and the item it is on when the key
        // waits for a second click.
        let (key, item) = if let Some(p) = &mut self.pipe {
            match (&mut p.job, hit) {
                _ if y == 0 => (Key::Char('q'), None),
                (Some(v), _) if panes(JOB_TOP) && x < 2 + pipeline::LW => {
                    v.steps = true;
                    let Some(i) = y.checked_sub(JOB_TOP).filter(|i| *i < n_steps) else { return vec![] };
                    v.step = i;
                    (Key::Enter, Some(i))
                }
                (Some(_), _) if panes(JOB_TOP) => (Key::Char('l'), None),
                (None, Some(k)) => {
                    p.sel = k;
                    (Key::Enter, hit)
                }
                _ => return vec![],
            }
        } else if let Some(d) = &mut self.detail {
            match (&mut d.picker, hit) {
                (Some((_, sel)), Some(r)) => {
                    *sel = r;
                    (Key::Enter, hit)
                }
                (Some(_), None) => return vec![],
                _ if y == 0 => (Key::Char('q'), None),
                _ if panes(detail::TOP) => (Key::Char(if x < 2 + detail::LW && !d.zoom { 'h' } else { 'l' }), None),
                _ => return vec![],
            }
        } else if y == 0 {
            if let Some(i) = self.tab_at(x) {
                self.active = i;
            }
            return vec![];
        } else {
            let Some(i) = hit.filter(|_| x < self.list_width()) else { return vec![] };
            self.section_mut().sel = i;
            (Key::Enter, hit)
        };
        match item {
            Some(i) if !self.twice(i, now) => vec![],
            _ => self.key(key, now),
        }
    }

    /// Whether this is the second click on item `i` in quick succession.
    fn twice(&mut self, i: usize, now: Instant) -> bool {
        let double = self.last_click.is_some_and(|(j, at)| j == i && now.duration_since(at) < DOUBLE);
        // Forgotten once used: the screen a double click opens has rows of
        // its own, and a third click must not open one of those.
        self.last_click = (!double).then_some((i, now));
        double
    }

    /// The wheel scrolls the pane under the pointer, as the arrow keys
    /// would if it had the focus, and leaves the focus where it was.
    fn wheel(&mut self, x: usize, down: bool, now: Instant) -> Vec<Effect> {
        let key = if down { Key::Down } else { Key::Up };
        let roll = |a: &mut App| (0..WHEEL).flat_map(|_| a.key(key, now)).collect();
        if let Some(v) = self.pipe.as_mut().and_then(|p| p.job.as_mut()) {
            let was = std::mem::replace(&mut v.steps, x < 2 + pipeline::LW);
            let out = roll(self);
            if let Some(v) = self.pipe.as_mut().and_then(|p| p.job.as_mut()) {
                v.steps = was;
            }
            return out;
        }
        if self.pipe.is_none()
            && let Some(d) = self.detail.as_mut().filter(|d| d.picker.is_none())
        {
            let over = if x < 2 + detail::LW && !d.zoom { Pane::Overview } else { Pane::Diff };
            let was = std::mem::replace(&mut d.focus, over);
            let out = roll(self);
            if let Some(d) = &mut self.detail {
                d.focus = was;
            }
            return out;
        }
        roll(self)
    }

    /// Where the list's rows end: the preview beside them is not a row.
    fn list_width(&self) -> usize {
        let w = self.last_width;
        if self.view_is(self.active, View::List) && self.preview_shown(w) { w.saturating_sub(PREVIEW_W + 4) } else { w }
    }

    /// Which tab covers column `x`, laid out as `views::chrome::tabs` lays
    /// them out.
    fn tab_at(&self, x: usize) -> Option<usize> {
        let short = self.narrow(self.last_width);
        let mut at = 2;
        for (i, s) in self.sections.iter().enumerate() {
            let name = if short { &s.section.short } else { &s.section.name };
            let w = name.chars().count() + 1 + s.total.to_string().len();
            if x >= at && x < at + w {
                return Some(i);
            }
            at += w + if short { 3 } else { 4 };
        }
        None
    }
}

#[cfg(test)]
mod tests;
