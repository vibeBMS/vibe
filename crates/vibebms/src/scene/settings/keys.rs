use super::{display, touch, Act, Row, Screen};
use crate::app::Ctx;
use crate::config::{KeyConfig, LaneKeys};
use crate::scene::keybind::{keys_label, Slot};
use crate::scene::option_panel::onoff;

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let k = &ctx.config.keys;
    let bind =
        |label: String, keys: &LaneKeys, s: Slot| Row::new(label, keys_label(keys), Act::Bind(s));
    let mut v: Vec<Row> = display::open_row(ctx, Screen::KeyConfig)
        .into_iter()
        .collect();
    for (side, sk) in [&k.p1, &k.p2].into_iter().enumerate() {
        let n = side + 1;
        for (idx, lk) in sk.keys.iter().enumerate().take(7) {
            v.push(bind(
                format!("{n}P key {}", idx + 1),
                lk,
                Slot::Key { side, idx },
            ));
        }
        v.push(bind(
            format!("{n}P scratch ↑"),
            &sk.scratch_up,
            Slot::ScratchUp(side),
        ));
        v.push(bind(
            format!("{n}P scratch ↓"),
            &sk.scratch_down,
            Slot::ScratchDown(side),
        ));
        let axes: Vec<String> = sk
            .scratch_axis
            .iter()
            .map(|a| format!("Pad{}#{}", a / 1_000_000, a % 1_000_000))
            .collect();
        v.push(Row::new(
            format!("{n}P turntable axis"),
            if axes.is_empty() {
                "—".into()
            } else {
                axes.join(", ")
            },
            Act::Bind(Slot::ScratchAxis(side)),
        ));
    }
    for (i, lk) in k.pms.iter().enumerate().take(9) {
        v.push(bind(format!("9K button {}", i + 1), lk, Slot::Pms(i)));
    }
    v.extend([
        bind("START".into(), &k.start, Slot::Start),
        bind("SELECT".into(), &k.select, Slot::Select),
        bind("2P START (VS)".into(), &k.p2_start, Slot::Start2),
        bind("2P SELECT (VS)".into(), &k.p2_select, Slot::Select2),
        Row::adjust(
            "Any gamepad axis = turntable",
            onoff(k.any_axis_is_scratch),
            |c, _| c.config.keys.any_axis_is_scratch ^= true,
        ),
        Row::new(
            "Touch controller layout",
            format!("{}  edit ↵", touch::style_label(ctx.config.touch.style)),
            Act::Open(Screen::TouchLayout),
        ),
        Row::new(
            "Reset key config",
            "↵",
            Act::Run(|c, _| c.config.keys = KeyConfig::default(), 0),
        ),
    ]);
    v
}
