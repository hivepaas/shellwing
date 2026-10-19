//! Finding text on the screen and in its history: plain text or a regular
//! expression, in any case or only the case typed.
//!
//! libghostty's own search finds plain text only, and ignores case for ASCII
//! letters only ("việt" does not find "VIỆT"). So this reads the rows itself
//! and runs `regex` over them, a logical line at a time (rows joined where
//! they wrapped), for a few milliseconds at a time between batches of output.
//!
//! Rows are counted from where the search began rather than from the top of
//! the history, which moves as old rows are dropped. `base` turns one count
//! into the other, kept right by a tracked reference to the last row, which
//! moves along with the text. Rows in the history do not change, so each is
//! read once. Rows of the active area can change, so they are read again
//! after every change. The search starts over after a resize (which reflows
//! the rows), on a switch to or from the alternate screen, and if the anchor
//! is lost.

use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};
use std::time::{Duration, Instant};

use libghostty_vt::Terminal;
use libghostty_vt::error::Result;
use libghostty_vt::screen::{CellWide, Screen, TrackedGridRef};
use libghostty_vt::terminal::{Point, PointCoordinate, PointSpace, ScrollViewport};
use regex::{Regex, RegexBuilder};

use crate::{FindStatus, Found};

/// What to find.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindQuery {
    pub text: String,
    /// Letters match only in the case typed.
    pub match_case: bool,
    /// `text` is a regular expression, in the syntax of Rust's `regex`.
    pub regex: bool,
}

impl FindQuery {
    /// The query as a pattern, or why it is not one.
    pub(crate) fn compile(&self) -> std::result::Result<Regex, regex::Error> {
        let pattern = if self.regex {
            self.text.clone()
        } else {
            regex::escape(&self.text)
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.match_case)
            .build()
    }
}

/// Matches kept at most; past that, the rest of the history is not read.
pub const MAX_MATCHES: usize = 10_000;
/// How far above the view a match may begin and still reach into it.
const LONGEST: i64 = 256;

/// A match ends here: its last row, and the column past its last cell.
#[derive(Clone, Copy, Debug)]
struct End {
    row: i64,
    col: u16,
}

/// One cell of a logical line: where its text begins, its row and column,
/// and its width.
type Cell = (usize, i64, u16, u16);

pub(crate) struct Finder {
    query: FindQuery,
    re: Regex,
    /// By first row and column, so in order from the oldest.
    hits: BTreeMap<(i64, u16), End>,
    /// The row counted from where the search began of screen row 0.
    base: i64,
    /// The last row as last seen, and its row counted from the beginning.
    anchor: Option<(TrackedGridRef, i64)>,
    /// Rows above this one (counted from the beginning) are still to read.
    older: i64,
    /// Rows from this one down are to read (again): new or changed.
    fresh: Option<i64>,
    /// The first row of the active area at the last look.
    active_top: i64,
    alternate: bool,
    size: (u16, u16),
    chosen: Option<(i64, u16)>,
    /// Choose the newest match once there is one: the query is new.
    choose: bool,
    capped: bool,
}

fn total_rows(term: &Terminal) -> Result<i64> {
    Ok(term.scrollbar()?.total as i64)
}

fn screen(y: i64) -> Point {
    Point::Screen(PointCoordinate {
        x: 0,
        y: y.max(0) as u32,
    })
}

/// Whether screen row `y` goes on in the next one.
fn wraps(term: &Terminal, y: i64) -> Result<bool> {
    term.grid_ref(screen(y))?.row()?.is_wrapped()
}

impl Finder {
    pub(crate) fn new(query: FindQuery, re: Regex, term: &mut Terminal) -> Result<Finder> {
        let mut f = Finder {
            query,
            re,
            hits: BTreeMap::new(),
            base: 0,
            anchor: None,
            older: 0,
            fresh: None,
            active_top: 0,
            alternate: false,
            size: (0, 0),
            chosen: None,
            choose: true,
            capped: false,
        };
        f.restart(term)?;
        Ok(f)
    }

    pub(crate) fn query(&self) -> &FindQuery {
        &self.query
    }

    /// Forgets every match and reads everything again, from the bottom up.
    fn restart(&mut self, term: &mut Terminal) -> Result<()> {
        self.hits.clear();
        self.chosen = None;
        self.capped = false;
        self.base = 0;
        self.size = (term.cols()?, term.rows()?);
        self.alternate = matches!(term.active_screen()?, Screen::Alternate);
        let total = total_rows(term)?;
        self.older = total;
        self.fresh = None;
        self.active_top = total - self.size.1 as i64;
        self.place_anchor(term, total)
    }

    /// Puts the anchor on the last row.
    fn place_anchor(&mut self, term: &mut Terminal, total: i64) -> Result<()> {
        let y = (total - 1).max(0);
        match &mut self.anchor {
            Some((a, row)) => {
                a.set(term, screen(y))?;
                *row = self.base + y;
            }
            None => self.anchor = Some((term.track_grid_ref(screen(y))?, self.base + y)),
        }
        Ok(())
    }

    /// Works `base` out again from where the anchor is now; false once it is
    /// gone.
    fn follow_anchor(&mut self) -> Result<bool> {
        let Some((a, row)) = &self.anchor else {
            return Ok(false);
        };
        match a.point(PointSpace::Screen)? {
            Some(p) => {
                self.base = *row - p.y as i64;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// The first row of the logical line that `row` is in.
    fn line_start(&self, term: &Terminal, row: i64) -> Result<i64> {
        let mut y = (row - self.base).max(0);
        while y > 0 && wraps(term, y - 1)? {
            y -= 1;
        }
        Ok(self.base + y)
    }

    pub(crate) fn searching(&self) -> bool {
        self.fresh.is_some() || (self.older > self.base && !self.capped)
    }

    /// Catches up with the terminal when it `changed`, and reads on for at
    /// most `budget`. Says whether there is news to show.
    pub(crate) fn advance(
        &mut self,
        term: &mut Terminal,
        changed: bool,
        budget: Duration,
    ) -> Result<bool> {
        let before = self.status();
        if changed {
            let size = (term.cols()?, term.rows()?);
            let alternate = matches!(term.active_screen()?, Screen::Alternate);
            if size != self.size || alternate != self.alternate || !self.follow_anchor()? {
                self.restart(term)?;
            } else {
                // Rows dropped from the top of the history (or cleared) take
                // their matches with them.
                self.hits = self.hits.split_off(&(self.base, 0));
                let total = total_rows(term)?;
                let top = self.base + total - size.1 as i64;
                // What was the active area at the last look may have changed
                // before it scrolled up into the history.
                let from = self.line_start(term, self.active_top.min(top))?;
                drop(self.hits.split_off(&(from, 0)));
                self.fresh = Some(self.fresh.map_or(from, |f| f.min(from)));
                self.older = self.older.min(from);
                self.active_top = top;
                self.place_anchor(term, total)?;
            }
        }
        let start = Instant::now();
        let end = self.base + total_rows(term)?;
        // New and changed rows first: they are on the screen.
        while let Some(from) = self.fresh
            && start.elapsed() < budget
        {
            if from >= end || self.capped {
                self.fresh = None;
            } else {
                self.fresh = Some(self.read_line(term, from)?);
            }
        }
        while self.older > self.base && !self.capped && start.elapsed() < budget {
            let first = self.line_start(term, self.older - 1)?;
            self.read_line(term, first)?;
            self.older = first;
        }
        if self.choose
            && let Some((&newest, _)) = self.hits.iter().next_back()
        {
            self.choose = false;
            self.chosen = Some(newest);
            self.reveal(term)?;
        }
        Ok(self.status() != before)
    }

    /// Reads the logical line beginning at `first` and keeps its matches;
    /// returns the row after it.
    fn read_line(&mut self, term: &Terminal, first: i64) -> Result<i64> {
        let total = total_rows(term)?;
        let mut text = String::new();
        let mut cells: Vec<Cell> = Vec::new();
        let mut g = ['\0'; 16];
        let mut y = (first - self.base).max(0);
        while y < total {
            let mut wrapped = false;
            for x in 0..self.size.0 {
                let r = term.grid_ref(Point::Screen(PointCoordinate { x, y: y as u32 }))?;
                if x == 0 {
                    wrapped = r.row()?.is_wrapped()?;
                }
                let cell = r.cell()?;
                let wide = cell.wide()?;
                if matches!(wide, CellWide::SpacerTail | CellWide::SpacerHead) {
                    continue;
                }
                let width = if matches!(wide, CellWide::Wide) { 2 } else { 1 };
                cells.push((text.len(), self.base + y, x, width));
                let n = if cell.has_text()? {
                    r.graphemes(&mut g).unwrap_or(0)
                } else {
                    0
                };
                if n == 0 {
                    text.push(' ');
                } else {
                    text.extend(&g[..n]);
                }
            }
            y += 1;
            if !wrapped {
                break;
            }
        }
        let at = |byte: usize| cells[cells.partition_point(|c| c.0 <= byte).saturating_sub(1)];
        for m in self.re.find_iter(&text) {
            if m.is_empty() {
                continue;
            }
            if self.hits.len() >= MAX_MATCHES {
                self.capped = true;
                break;
            }
            let (a, b) = (at(m.start()), at(m.end() - 1));
            self.hits.insert(
                (a.1, a.2),
                End {
                    row: b.1,
                    col: b.2 + b.3,
                },
            );
        }
        Ok(self.base + y)
    }

    pub(crate) fn status(&self) -> FindStatus {
        let current = self
            .chosen
            .filter(|k| self.hits.contains_key(k))
            .map(|k| self.hits.range((Excluded(k), Unbounded)).count());
        FindStatus {
            total: self.hits.len(),
            current,
            searching: self.searching(),
            capped: self.capped,
            invalid: false,
        }
    }

    /// Chooses the next match, `older` (up) or newer, wrapping round at either
    /// end; with none chosen, the newest.
    pub(crate) fn step(&mut self, term: &mut Terminal, older: bool) -> Result<()> {
        self.choose = false;
        let next = match self.chosen.filter(|k| self.hits.contains_key(k)) {
            Some(k) if older => self
                .hits
                .range(..k)
                .next_back()
                .or_else(|| self.hits.iter().next_back()),
            Some(k) => self
                .hits
                .range((Excluded(k), Unbounded))
                .next()
                .or_else(|| self.hits.iter().next()),
            None => self.hits.iter().next_back(),
        };
        if let Some((&k, _)) = next {
            self.chosen = Some(k);
            self.reveal(term)?;
        }
        Ok(())
    }

    /// Scrolls the chosen match into the middle of the view when it is not
    /// in view.
    fn reveal(&self, term: &mut Terminal) -> Result<()> {
        let Some(k) = self.chosen else {
            return Ok(());
        };
        let Some(end) = self.hits.get(&k) else {
            return Ok(());
        };
        let sb = term.scrollbar()?;
        let (top, len) = (sb.offset as i64, sb.len as i64);
        let (first, last) = (k.0 - self.base, end.row - self.base);
        if first >= top && last < top + len {
            return Ok(());
        }
        let row = (first - len / 2).max(0) as usize;
        term.scroll_viewport(ScrollViewport::Row(row));
        Ok(())
    }

    /// The matches in view, as column ranges for each row.
    pub(crate) fn spans(&self, term: &Terminal) -> Result<Vec<Vec<(u16, u16, Found)>>> {
        let sb = term.scrollbar()?;
        let rows = self.size.1 as i64;
        let top = self.base + sb.offset as i64;
        let bottom = top + rows;
        let mut out = vec![Vec::new(); rows.max(0) as usize];
        for (&(row, col), end) in self.hits.range((top - LONGEST, 0)..(bottom, 0)) {
            if end.row < top {
                continue;
            }
            let found = if self.chosen == Some((row, col)) {
                Found::Current
            } else {
                Found::Match
            };
            for r in row.max(top)..=end.row.min(bottom - 1) {
                let start = if r == row { col } else { 0 };
                let stop = if r == end.row { end.col } else { self.size.0 };
                out[(r - top) as usize].push((start, stop, found));
            }
        }
        Ok(out)
    }
}
