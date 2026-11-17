use super::{Act, Row, TextField};
use crate::app::Ctx;
use crate::scene::option_panel::onoff;

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let ir = &ctx.config.ir_server;
    vec![
        Row::adjust("Internet ranking", onoff(ir.enabled), |c, _| {
            c.config.ir_server.enabled ^= true
        }),
        Row::new("Name", ir.name.clone(), Act::Text(TextField::IrName)),
        Row::new("URL", ir.url.clone(), Act::Text(TextField::IrUrl)),
        Row::new(
            "API token",
            if ir.token.is_empty() {
                "—"
            } else {
                "••••••••"
            },
            Act::Text(TextField::IrToken),
        ),
        Row::adjust("Auto submit scores", onoff(ir.auto_submit), |c, _| {
            c.config.ir_server.auto_submit ^= true
        }),
        Row::note("Protocol: docs/IR_PROTOCOL.md", ""),
    ]
}
