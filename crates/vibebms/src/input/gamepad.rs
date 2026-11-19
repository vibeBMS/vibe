//! Game controllers (gilrs; on the web the browser's Gamepad API, which the
//! browser samples about once a frame, so a button's time is that coarse).
//! Android and iOS have none here.
//!
//! Each controller has a number, the `pad` of its bindings
//! (`pad * 1_000_000 + code`): 0 is the 1P controller, 1 the 2P one. They
//! are the controllers chosen in `keys.controllers` when connected
//! ([`numbers`]); without a choice, controllers are numbered in the order
//! they were connected.

use super::InputEv;
use crate::config::{PadId, PadSides};

/// A controller seen this launch.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub pad: PadId,
    pub connected: bool,
}

/// A connected controller and its number (0: 1P, 1: 2P), for the
/// settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Connected {
    pub pad: PadId,
    pub number: u64,
}

/// How well a controller is the one chosen: 2 its name and id, 1 one of
/// them (the id only when there is one: a browser tells none), 0 not.
fn likeness(seen: &PadId, want: &PadId) -> u8 {
    let id = !want.id.is_empty() && seen.id == want.id;
    let name = seen.name == want.name;
    match (id, name) {
        (true, true) => 2,
        _ if want.id.is_empty() && name => 2,
        (true, false) | (false, true) => 1,
        _ => 0,
    }
}

/// The numbers of `pads` (in connection order) for the choice `want`:
/// each side's controller is the one most like the one chosen (name and id
/// before either; connected before not; the earliest connected first, so
/// two of the same model are 1P and 2P in connection order). The others,
/// and a side no controller is like, follow in connection order: without a
/// choice the numbers are the connection order.
pub fn numbers(pads: &[Seen], want: &PadSides) -> Vec<u64> {
    let mut num: Vec<Option<u64>> = vec![None; pads.len()];
    let mut taken = [false; 2];
    for strength in [2, 1] {
        for connected in [true, false] {
            for (side, t) in taken.iter_mut().enumerate() {
                let Some(w) = want.get(side) else { continue };
                if *t {
                    continue;
                }
                let found = pads.iter().enumerate().position(|(i, p)| {
                    num[i].is_none() && p.connected == connected && likeness(&p.pad, w) == strength
                });
                if let Some(i) = found {
                    num[i] = Some(side as u64);
                    *t = true;
                }
            }
        }
    }
    let mut free = (0u64..).filter(|&n| n >= 2 || !taken[n as usize]);
    num.into_iter()
        .map(|n| n.unwrap_or_else(|| free.next().unwrap()))
        .collect()
}

pub struct Gamepads {
    #[cfg(gamepads)]
    gilrs: Option<gilrs::Gilrs>,
    /// The controllers seen, by gilrs' id (its index: connection order).
    #[cfg(gamepads)]
    seen: Vec<Seen>,
    /// Their numbers, made for `sides`.
    #[cfg(gamepads)]
    numbers: Vec<u64>,
    #[cfg(gamepads)]
    sides: PadSides,
    /// The connected controllers changed or were numbered again: for the
    /// settings (`take_connected`).
    changed: bool,
}

/// A controller's name and id as gilrs tells them (the id: USB vendor and
/// product).
#[cfg(gamepads)]
fn pad_id(g: &gilrs::Gamepad) -> PadId {
    let id = match (g.vendor_id(), g.product_id()) {
        (Some(v), Some(p)) => format!("{v:04x}:{p:04x}"),
        _ => {
            let u = g.uuid();
            if u.iter().all(|b| *b == 0) {
                String::new()
            } else {
                vibebms_core::hex(&u)
            }
        }
    };
    PadId {
        name: g.name().trim().to_string(),
        id,
    }
}

impl Gamepads {
    /// The controllers connected now and later.
    pub fn open() -> Gamepads {
        #[cfg(gamepads)]
        {
            let gilrs = gilrs::Gilrs::new()
                .map_err(|e| log::warn!("gamepads unavailable: {e}"))
                .ok();
            let mut g = Gamepads {
                gilrs,
                seen: Vec::new(),
                numbers: Vec::new(),
                sides: PadSides::default(),
                changed: true,
            };
            let ids: Vec<gilrs::GamepadId> = g
                .gilrs
                .iter()
                .flat_map(|gi| gi.gamepads().map(|(id, _)| id))
                .collect();
            for id in ids {
                g.see(id, true);
            }
            g
        }
        #[cfg(not(gamepads))]
        Gamepads { changed: false }
    }

    /// Controller `gid` is connected.
    #[cfg(gamepads)]
    fn see(&mut self, gid: gilrs::GamepadId, connected: bool) {
        let Some(gi) = self.gilrs.as_ref() else {
            return;
        };
        let pad = pad_id(&gi.gamepad(gid));
        let id = usize::from(gid);
        if self.seen.len() <= id {
            self.seen.resize(
                id + 1,
                Seen {
                    pad: PadId::default(),
                    connected: false,
                },
            );
        }
        self.seen[id] = Seen { pad, connected };
        self.renumber();
    }

    #[cfg(gamepads)]
    fn renumber(&mut self) {
        self.numbers = if self.sides.is_default() {
            // as before the choice: gilrs' own order
            (0..self.seen.len() as u64).collect()
        } else {
            numbers(&self.seen, &self.sides)
        };
        self.changed = true;
    }

    /// The connected controllers by number, once they changed since the
    /// last call.
    pub fn take_connected(&mut self) -> Option<Vec<Connected>> {
        if !std::mem::take(&mut self.changed) {
            return None;
        }
        #[cfg(gamepads)]
        {
            let mut v: Vec<Connected> = self
                .seen
                .iter()
                .zip(&self.numbers)
                .filter(|(s, _)| s.connected)
                .map(|(s, n)| Connected {
                    pad: s.pad.clone(),
                    number: *n,
                })
                .collect();
            v.sort_by_key(|c| c.number);
            Some(v)
        }
        #[cfg(not(gamepads))]
        Some(Vec::new())
    }

    /// The controllers' input since the last call, numbered for the choice
    /// `sides`; `toast` is told when one is connected or gone.
    #[cfg(gamepads)]
    pub fn poll(&mut self, sides: &PadSides, mut toast: impl FnMut(String)) -> Vec<InputEv> {
        use crate::platform::{Duration, Instant};
        if *sides != self.sides {
            self.sides = sides.clone();
            self.renumber();
        }
        let mut evs = Vec::new();
        let now_i = Instant::now();
        // (gilrs stamps with std's clock, which the web can read but not
        // ask for the time: compare in Unix milliseconds)
        let now_ms = crate::platform::unix_ms();
        while let Some(e) = self.gilrs.as_mut().and_then(|g| g.next_event()) {
            let at_ms = e
                .time
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(now_ms, |d| d.as_millis() as i64);
            let age = Duration::from_millis(now_ms.saturating_sub(at_ms).clamp(0, 50) as u64);
            let time = now_i.checked_sub(age).unwrap_or(now_i);
            let raw = usize::from(e.id);
            let number = |g: &Gamepads| g.numbers.get(raw).copied().unwrap_or(raw as u64);
            let pad = number(self) * 1_000_000;
            match e.event {
                gilrs::EventType::ButtonPressed(_, code) => evs.push(InputEv::Pad {
                    id: pad + code.into_u32() as u64,
                    pressed: true,
                    time,
                }),
                gilrs::EventType::ButtonReleased(_, code) => evs.push(InputEv::Pad {
                    id: pad + code.into_u32() as u64,
                    pressed: false,
                    time,
                }),
                gilrs::EventType::AxisChanged(_, value, code) => evs.push(InputEv::Axis {
                    id: pad + code.into_u32() as u64,
                    value,
                    time,
                }),
                gilrs::EventType::Connected => {
                    self.see(e.id, true);
                    let name = &self.seen[raw].pad.name;
                    toast(tr!(
                        "Controller connected: {name} ({side})",
                        name = name,
                        side = side_name(number(self))
                    ));
                }
                gilrs::EventType::Disconnected => {
                    let name = self.seen.get(raw).map(|s| s.pad.name.clone());
                    if let Some(s) = self.seen.get_mut(raw) {
                        s.connected = false;
                        self.renumber();
                    }
                    toast(tr!(
                        "Controller disconnected: {name}",
                        name = name.unwrap_or_default()
                    ));
                }
                _ => {}
            }
        }
        evs
    }

    #[cfg(not(gamepads))]
    pub fn poll(&mut self, _sides: &PadSides, _toast: impl FnMut(String)) -> Vec<InputEv> {
        Vec::new()
    }
}

/// A controller number as the player knows it: 1P, 2P, then the
/// binding's `Pad<n>`.
pub fn side_name(number: u64) -> String {
    match number {
        0 => "1P".into(),
        1 => "2P".into(),
        n => format!("Pad{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(name: &str, id: &str, connected: bool) -> Seen {
        Seen {
            pad: PadId {
                name: name.into(),
                id: id.into(),
            },
            connected,
        }
    }

    fn want(p1: Option<(&str, &str)>, p2: Option<(&str, &str)>) -> PadSides {
        let id = |(n, i): (&str, &str)| PadId {
            name: n.into(),
            id: i.into(),
        };
        PadSides {
            p1: p1.map(id),
            p2: p2.map(id),
        }
    }

    #[test]
    fn without_a_choice_connection_order() {
        let pads = [
            pad("A", "1:1", true),
            pad("B", "2:2", true),
            pad("C", "", true),
        ];
        assert_eq!(numbers(&pads, &PadSides::default()), [0, 1, 2]);
    }

    #[test]
    fn the_chosen_controllers_are_1p_and_2p() {
        let pads = [
            pad("A", "1:1", true),
            pad("B", "2:2", true),
            pad("C", "3:3", true),
        ];
        // 1P chosen: it is 0, the others in order
        let w = want(Some(("C", "3:3")), None);
        assert_eq!(numbers(&pads, &w), [1, 2, 0]);
        // both chosen
        let w = want(Some(("B", "2:2")), Some(("A", "1:1")));
        assert_eq!(numbers(&pads, &w), [1, 0, 2]);
        // 2P only: 1P is the first other in connection order
        let w = want(None, Some(("A", "1:1")));
        assert_eq!(numbers(&pads, &w), [1, 0, 2]);
    }

    #[test]
    fn absent_controllers_leave_connection_order() {
        let pads = [pad("A", "1:1", true), pad("B", "2:2", true)];
        let w = want(Some(("X", "9:9")), Some(("Y", "8:8")));
        assert_eq!(numbers(&pads, &w), [0, 1]);
        // 1P absent, 2P here: the other is 1P
        let w = want(Some(("X", "9:9")), Some(("A", "1:1")));
        assert_eq!(numbers(&pads, &w), [1, 0]);
        assert!(numbers(&[], &w).is_empty());
    }

    #[test]
    fn name_and_id_before_either() {
        // the same model twice (same name and id), and one renamed
        let pads = [
            pad("IIDX", "1ccf:8048", true),
            pad("IIDX", "1ccf:8048", true),
            pad("PHOENIXWAN", "1ccf:8048", true),
        ];
        let w = want(Some(("PHOENIXWAN", "1ccf:8048")), None);
        assert_eq!(numbers(&pads, &w), [1, 2, 0]);
        // the same identity for both sides: connection order among them
        let w = want(Some(("IIDX", "1ccf:8048")), Some(("IIDX", "1ccf:8048")));
        assert_eq!(numbers(&pads, &w), [0, 1, 2]);
        // only the id the same (another name on another system): taken
        let w = want(None, Some(("Other name", "1ccf:8048")));
        assert_eq!(numbers(&pads, &w)[0], 1);
        // a browser tells no id: the name is enough
        let web = [pad("Pad A", "", true), pad("Pad B", "", true)];
        let w = want(Some(("Pad B", "")), None);
        assert_eq!(numbers(&web, &w), [1, 0]);
    }

    #[test]
    fn connected_before_not() {
        // the chosen 1P controller was unplugged and one like it plugged in
        let pads = [
            pad("A", "1:1", false),
            pad("B", "2:2", true),
            pad("A", "1:1", true),
        ];
        let w = want(Some(("A", "1:1")), None);
        assert_eq!(numbers(&pads, &w), [1, 2, 0]);
        // gone with no other like it: it keeps its number (no mid-play swap)
        let pads = [pad("A", "1:1", false), pad("B", "2:2", true)];
        assert_eq!(numbers(&pads, &w), [0, 1]);
    }
}
