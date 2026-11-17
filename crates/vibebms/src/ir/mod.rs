//! Internet ranking: what the app keeps and sends for the IR servers
//! (`vibebms-ir` has the protocols).

pub mod build_id;
pub mod creds;
pub mod store;

use crate::config::{IrAccount, IrSettings, Provider};
use crate::paths::Paths;
use vibebms_core::record::PlayRecord;

/// The IR's state: the history of plays, the accounts' keys.
pub struct IrService {
    history: store::History,
    pub creds: creds::Creds,
}

impl IrService {
    /// The service over the files in `paths` (`now_ms`: Unix ms).
    pub fn new(paths: &Paths, now_ms: i64) -> IrService {
        let mut history = store::History::open(paths.history.clone(), now_ms);
        history.compact(now_ms);
        IrService {
            history,
            creds: creds::Creds::open(paths),
        }
    }

    /// Keys an older configuration kept go to the credentials store; true
    /// when one did (the configuration is then saved without it).
    pub fn take_old_tokens(&mut self, ir: &mut IrSettings) -> bool {
        let mut moved = false;
        for a in ir.accounts.iter_mut().filter(|a| !a.old_token.0.is_empty()) {
            match self.creds.set(&a.key(), Some(a.old_token.0.clone())) {
                Ok(()) => {
                    a.old_token.0.clear();
                    moved = true;
                }
                Err(e) => log::warn!("credentials: {e}"),
            }
        }
        moved
    }

    /// The client of a vibeBMS IR account, with its key.
    pub fn vibe_json(&self, a: &IrAccount) -> Option<vibebms_ir::JsonIr> {
        (a.provider == Provider::VibeJson).then_some(())?;
        vibebms_ir::JsonIr::new(&vibebms_ir::IrConfig {
            enabled: a.enabled,
            name: a.name.clone(),
            url: a.url.clone(),
            token: self.creds.get(&a.key()).unwrap_or_default().to_string(),
            auto_submit: a.submit,
        })
    }

    /// A play has ended: it gets its play id and goes into the history
    /// (a play of the player's own: not autoplay, not a replay watched).
    pub fn record(&mut self, rec: &mut PlayRecord) {
        if rec.flags.autoplay || rec.flags.replay {
            return;
        }
        if let Err(e) = self.history.add(rec) {
            log::warn!("play history: {e}");
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::config::Config;

    /// An `[ir_server]` of before: its token leaves config.toml for the
    /// credentials store the first time the app starts.
    #[test]
    fn the_old_token_leaves_the_config() {
        let root = std::env::temp_dir().join(format!("vibebms-ir-old-{}", std::process::id()));
        let paths = Paths::at(root.clone());
        let file = paths.config_file();
        std::fs::write(
            &file,
            "[ir_server]\nenabled = true\nurl = \"https://ir.example\"\ntoken = \"s3cr3t\"\n",
        )
        .unwrap();
        let mut config = Config::load(&file);
        let mut ir = IrService::new(&paths, 0);
        assert!(ir.take_old_tokens(&mut config.ir));
        config.save(&file);
        assert!(!std::fs::read_to_string(&file).unwrap().contains("s3cr3t"));
        let a = &Config::load(&file).ir.accounts[0];
        let again = IrService::new(&paths, 0);
        assert_eq!(again.creds.get(&a.key()), Some("s3cr3t"));
        assert!(again.vibe_json(a).is_some());
        std::fs::remove_dir_all(&root).ok();
    }
}
