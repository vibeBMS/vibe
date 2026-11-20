//! Control flow: `#RANDOM` / `#SETRANDOM` / `#IF` / `#ELSEIF` / `#ELSE` /
//! `#ENDIF` / `#ENDRANDOM` and `#SWITCH` / `#SETSWITCH` / `#CASE` / `#SKIP` /
//! `#DEF` / `#ENDSW`, nested.

use super::{parse_i32, ParseOptions};
use crate::rng::JavaRandom;

#[derive(Debug)]
enum Frame {
    /// (`active`: whether the lines around it are read, so that it need
    /// not be looked up in the blocks below)
    Random { active: bool },
    If {
        active: bool,
        matched: bool,
        parent: bool,
        depth: usize,
    },
    Switch {
        value: u32,
        active: bool,
        matched: bool,
        skipping: bool,
        parent: bool,
    },
}

/// The `#RANDOM` / `#SWITCH` blocks open at a line of the text.
pub(super) struct Flow<'a> {
    stack: Vec<Frame>,
    /// The value of each `#RANDOM` open (a `Frame::Random` each).
    randoms: Vec<u32>,
    pub rng: JavaRandom,
    forced: Option<&'a [u32]>,
    forced_idx: usize,
    /// The outcome of each `#RANDOM` / `#SWITCH` rolled, in order.
    pub selections: Vec<u32>,
    /// A `#RANDOM`, `#SETRANDOM`, `#SWITCH` or `#SETSWITCH` was met.
    pub used: bool,
}

impl Flow<'_> {
    pub fn new(opts: &ParseOptions) -> Flow<'_> {
        Flow {
            stack: Vec::new(),
            randoms: Vec::new(),
            rng: JavaRandom::new(opts.random_seed),
            forced: opts.random_selections.as_deref(),
            forced_idx: 0,
            selections: Vec::new(),
            used: false,
        }
    }

    /// Whether the lines here are read (not in a branch not taken).
    pub fn active(&self) -> bool {
        match self.stack.last() {
            Some(
                Frame::Random { active } | Frame::If { active, .. } | Frame::Switch { active, .. },
            ) => *active,
            None => true,
        }
    }

    /// How many blocks are open.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    fn current_random(&self) -> u32 {
        self.randoms.last().copied().unwrap_or(0)
    }

    fn pop(&mut self) -> Option<Frame> {
        let f = self.stack.pop();
        if let Some(Frame::Random { .. }) = f {
            self.randoms.pop();
        }
        f
    }

    fn roll(&mut self, max: u32) -> u32 {
        self.used = true;
        let v = if let Some(f) = self.forced.and_then(|f| f.get(self.forced_idx)) {
            *f
        } else {
            self.rng.next_int_bounded(max.max(1) as i32) as u32 + 1
        };
        self.forced_idx += 1;
        self.selections.push(v);
        v
    }

    /// Returns true when the line was a control-flow directive.
    pub fn handle(&mut self, key: &str, arg: &str) -> bool {
        let n = || parse_i32(arg).unwrap_or(0).max(0) as u32;
        match key {
            "RANDOM" | "SETRANDOM" => {
                let active = self.active();
                let value = if !active {
                    0
                } else if key == "RANDOM" {
                    self.roll(n())
                } else {
                    self.used = true;
                    n()
                };
                self.randoms.push(value);
                self.stack.push(Frame::Random { active });
            }
            "ENDRANDOM" => {
                while let Some(f) = self.pop() {
                    if matches!(f, Frame::Random { .. }) {
                        break;
                    }
                }
            }
            "IF" => {
                let depth = self.randoms.len();
                if let Some(Frame::If { depth: d, .. }) = self.stack.last() {
                    if *d == depth {
                        self.stack.pop(); // implicit #ENDIF
                    }
                }
                let parent = self.active();
                let hit = self.current_random() == n();
                self.stack.push(Frame::If {
                    active: parent && hit,
                    matched: hit,
                    parent,
                    depth,
                });
            }
            "ELSEIF" => {
                let r = self.current_random();
                if let Some(Frame::If {
                    active,
                    matched,
                    parent,
                    ..
                }) = self.stack.last_mut()
                {
                    let hit = !*matched && r == n();
                    *active = *parent && hit;
                    *matched |= hit;
                }
            }
            "ELSE" => {
                if let Some(Frame::If {
                    active,
                    matched,
                    parent,
                    ..
                }) = self.stack.last_mut()
                {
                    *active = *parent && !*matched;
                    *matched = true;
                }
            }
            "ENDIF" => {
                if matches!(self.stack.last(), Some(Frame::If { .. })) {
                    self.stack.pop();
                }
            }
            "SWITCH" | "SETSWITCH" => {
                let parent = self.active();
                let value = if !parent {
                    0
                } else if key == "SWITCH" {
                    self.roll(n())
                } else {
                    self.used = true;
                    n()
                };
                self.stack.push(Frame::Switch {
                    value,
                    active: false,
                    matched: false,
                    skipping: false,
                    parent,
                });
            }
            "CASE" => {
                let v = n();
                if let Some(Frame::Switch {
                    value,
                    active,
                    matched,
                    skipping,
                    parent,
                }) = self.stack.last_mut()
                {
                    if !*skipping && !*active && *value == v {
                        *active = *parent;
                        *matched = true;
                    }
                }
            }
            "SKIP" => {
                if let Some(Frame::Switch {
                    active, skipping, ..
                }) = self.stack.last_mut()
                {
                    if *active {
                        *active = false;
                        *skipping = true;
                    }
                }
            }
            "DEF" => {
                if let Some(Frame::Switch {
                    active,
                    matched,
                    skipping,
                    parent,
                    ..
                }) = self.stack.last_mut()
                {
                    if !*skipping && !*matched {
                        *active = *parent;
                        *matched = true;
                    }
                }
            }
            "ENDSW" => {
                while let Some(f) = self.pop() {
                    if matches!(f, Frame::Switch { .. }) {
                        break;
                    }
                }
            }
            _ => return false,
        }
        true
    }
}
