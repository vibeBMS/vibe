//! Game controllers (gilrs). The web, Android and iOS have none here.

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
        use crate::platform::{Duration, Instant, SystemTime};
        let mut evs = Vec::new();
        let Some(g) = self.gilrs.as_mut() else {
            return evs;
        };
        let now_i = Instant::now();
        let now_s = SystemTime::now();
        while let Some(e) = g.next_event() {
            let age = now_s
                .duration_since(e.time)
                .unwrap_or_default()
                .min(Duration::from_millis(50));
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
