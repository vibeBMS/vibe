//! Game controllers (gilrs; on the web the browser's Gamepad API, which the
//! browser samples about once a frame, so a button's time is that coarse).
//! Android and iOS have none here.

use super::InputEv;

pub struct Gamepads {
    #[cfg(gamepads)]
    gilrs: Option<gilrs::Gilrs>,
}

impl Gamepads {
    /// The controllers connected now and later.
    pub fn open() -> Gamepads {
        Gamepads {
            #[cfg(gamepads)]
            gilrs: gilrs::Gilrs::new()
                .map_err(|e| log::warn!("gamepads unavailable: {e}"))
                .ok(),
        }
    }

    /// The controllers' input since the last call; `toast` is told when one
    /// is connected or gone.
    #[cfg(gamepads)]
    pub fn poll(&mut self, mut toast: impl FnMut(&'static str)) -> Vec<InputEv> {
        use crate::platform::{Duration, Instant};
        let mut evs = Vec::new();
        let Some(g) = self.gilrs.as_mut() else {
            return evs;
        };
        let now_i = Instant::now();
        // (gilrs stamps with std's clock, which the web can read but not
        // ask for the time: compare in Unix milliseconds)
        let now_ms = crate::platform::unix_ms();
        while let Some(e) = g.next_event() {
            let at_ms = e
                .time
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(now_ms, |d| d.as_millis() as i64);
            let age = Duration::from_millis(now_ms.saturating_sub(at_ms).clamp(0, 50) as u64);
            let time = now_i.checked_sub(age).unwrap_or(now_i);
            let pad = usize::from(e.id) as u64 * 1_000_000;
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
                gilrs::EventType::Connected => toast("Controller connected"),
                gilrs::EventType::Disconnected => toast("Controller disconnected"),
                _ => {}
            }
        }
        evs
    }

    #[cfg(not(gamepads))]
    pub fn poll(&mut self, _toast: impl FnMut(&'static str)) -> Vec<InputEv> {
        Vec::new()
    }
}
