//! Which rules start: the triggers looked at, and what is remembered of those that hold.

use super::*;

/// What is remembered of a trigger that holds, between two looks.
#[derive(Debug, Clone, Copy)]
pub(super) struct Watch {
    /// The rule it was for (an edited rule starts again).
    updated_at: i64,
    /// Since when it holds.
    since: Option<i64>,
    /// The rule started for this episode.
    fired: bool,
    /// Seen at least once (a state or a façade must be seen changing).
    seen: bool,
    /// The history already tells why the rule did not start in this episode.
    skipped: bool,
}

/// Which actions run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branch {
    Then,
    Else,
    /// A trigger started the rule, but its conditions were false and it has no "else".
    Skipped,
}

impl Branch {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Branch::Then => "then",
            Branch::Else => "else",
            Branch::Skipped => "skipped",
        }
    }
}

/// A rule a trigger started.
#[derive(Debug, Clone)]
pub struct Due {
    pub id: i64,
    pub branch: Branch,
    pub reason: Text,
    /// The trigger that holds and started it (its end runs the "over" steps).
    pub lasting: Option<usize>,
}

/// Decides which rules start. Kept in memory: after a restart, a measure already past its
/// threshold counts as a new episode.
#[derive(Debug)]
pub struct Engine {
    pub(super) watches: HashMap<(i64, usize), Watch>,
    /// When the rules were last looked at (time and sun triggers start once between two looks).
    pub(super) checked: i64,
}

impl Engine {
    pub fn new(checked: i64) -> Self {
        Self { watches: HashMap::new(), checked }
    }

    /// The rules to start now, each with its branch and what started it.
    pub fn due(&mut self, rules: &[(AutomationRow, Automation)], ctx: &Context) -> Vec<Due> {
        let mut out = vec![];
        self.watches.retain(|(id, _), _| rules.iter().any(|(row, _)| row.id == *id && row.enabled));
        for (row, rule) in rules.iter().filter(|(row, _)| row.enabled) {
            // Its sun and its weather are those of the installation it is about.
            let here = ctx.at(rule.place(ctx));
            let ctx = &here;
            // The triggers that start the rule now: (index, holds for a while).
            let mut started: Vec<(usize, bool)> = vec![];
            for (i, t) in rule.triggers.iter().enumerate() {
                let fresh = Watch { updated_at: row.updated_at, since: None, fired: false, seen: false, skipped: false };
                let w = self.watches.entry((row.id, i)).or_insert(fresh);
                if w.updated_at != row.updated_at {
                    *w = fresh;
                }
                // What cannot be read changes nothing.
                match t.look(ctx, self.checked) {
                    None => {}
                    Some(Look::Event(happened)) => {
                        if happened {
                            started.push((i, false));
                        }
                    }
                    Some(Look::Holds { holds, released, edge, for_minutes }) => {
                        if !w.seen {
                            w.seen = true;
                            // Already so when first seen: not a change.
                            w.fired = edge && holds;
                        }
                        if w.fired {
                            if released {
                                *w = Watch { seen: true, ..fresh };
                            }
                            continue;
                        }
                        if !holds {
                            w.since = None;
                            w.skipped = false;
                            continue;
                        }
                        let since = *w.since.get_or_insert(ctx.now);
                        if ctx.now - since >= for_minutes * 60 {
                            started.push((i, true));
                        }
                    }
                }
            }
            let Some(&(first, _)) = started.first() else { continue };
            if row.last_run_at.is_some_and(|t| ctx.now - t < MIN_GAP) {
                continue;
            }
            let reason = ctx.reason(&rule.triggers[first]);
            let branch = match rule.met(ctx) {
                Some(true) => Branch::Then,
                Some(false) if !rule.otherwise.is_empty() => Branch::Else,
                // A trigger that holds tries the conditions again while it holds: the history
                // tells once why the rule did not start.
                met => {
                    let holding: Vec<usize> = started.iter().filter(|(_, h)| *h).map(|(i, _)| *i).collect();
                    let told = !holding.is_empty() && holding.iter().all(|i| self.watches.get(&(row.id, *i)).is_some_and(|w| w.skipped));
                    for i in &holding {
                        if let Some(w) = self.watches.get_mut(&(row.id, *i)) {
                            w.skipped = true;
                        }
                    }
                    let waits = met.is_none() && !holding.is_empty();
                    if !told && !waits {
                        let why = ctx.why_not(rule);
                        out.push(Due {
                            id: row.id, branch: Branch::Skipped, reason: crate::text!("{0} — {1}", "{0} — {1}", "{0} — {1}", reason.arg(), why.arg()), lasting: None
                        });
                    }
                    continue;
                }
            };
            for (i, holding) in &started {
                if *holding {
                    if let Some(w) = self.watches.get_mut(&(row.id, *i)) {
                        w.fired = true;
                    }
                }
            }
            let lasting = started.iter().find(|(_, holding)| *holding).map(|(i, _)| *i);
            out.push(Due { id: row.id, branch, reason, lasting });
        }
        self.checked = ctx.now;
        out
    }
}
