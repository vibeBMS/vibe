//! TARGET: the score the pacemaker and the result compare against. Besides
//! the DJ level steps, MAX and the personal best it can be a rival: an IR
//! rival, one whose lamps an enabled account follows, at the EX score the
//! IR has of theirs for the chart; or a local rival, another profile, at
//! its best EX score of the chart (under the LN and rule modes chosen).
//! The IRs give a final EX score only, and so does a record, so the
//! rival's pacemaker runs at the share of the notes judged.

use crate::config::{PlayConfig, PlayerKey, Target};
use crate::ctx::Ctx;
use crate::library::Song;

/// Who a rival is.
#[derive(Clone, Debug, PartialEq)]
pub enum RivalKey {
    /// A player an IR account follows.
    Ir(PlayerKey),
    /// Another profile (its id).
    Local(String),
}

/// A rival TARGET offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Rival {
    pub key: RivalKey,
    pub name: String,
}

/// The rivals the enabled accounts follow, as the IR names them, then the
/// other profiles.
pub fn rivals(ctx: &Ctx) -> Vec<Rival> {
    let ir = &ctx.net.ir;
    let mut v: Vec<Rival> = (ctx.config.ir.accounts.iter().filter(|a| a.enabled))
        .flat_map(|a| a.rivals.iter().map(move |r| (a.provider, r)))
        .filter(|(_, r)| !r.is_empty())
        .map(|(provider, player)| Rival {
            name: ir
                .player_name(provider, player)
                .unwrap_or(player)
                .to_string(),
            key: RivalKey::Ir(PlayerKey {
                provider,
                player: player.clone(),
            }),
        })
        .collect();
    v.extend(ctx.profiles.rivals().map(|p| Rival {
        key: RivalKey::Local(p.id.clone()),
        name: p.name.clone(),
    }));
    v
}

/// The rival TARGET chose, when it is RIVAL and the rival is still one.
fn chosen<'a>(p: &PlayConfig, rivals: &'a [Rival]) -> Option<&'a Rival> {
    if p.target != Target::Rival {
        return None;
    }
    let k = match (&p.target_profile, &p.target_rival) {
        (Some(id), _) => RivalKey::Local(id.clone()),
        (None, Some(k)) => RivalKey::Ir(k.clone()),
        (None, None) => return None,
    };
    rivals.iter().find(|r| r.key == k)
}

/// TARGET moved `d` places along the targets, then the rivals, around the
/// end; its label.
pub fn step(p: &mut PlayConfig, rivals: &[Rival], d: i32) -> String {
    let fixed = &Target::ALL[..Target::ALL.len() - 1];
    let n = (fixed.len() + rivals.len()) as i32;
    let at = match (p.target, chosen(p, rivals)) {
        (Target::Rival, Some(r)) => fixed.len() + rivals.iter().position(|x| x == r).unwrap_or(0),
        (Target::Rival, None) => fixed.len(),
        (t, _) => fixed.iter().position(|x| *x == t).unwrap_or(0),
    } as i32;
    let i = (at + d).rem_euclid(n) as usize;
    match rivals.get(i.wrapping_sub(fixed.len())) {
        Some(r) => {
            p.target = Target::Rival;
            match &r.key {
                RivalKey::Ir(k) => {
                    p.target_rival = Some(k.clone());
                    p.target_profile = None;
                }
                RivalKey::Local(id) => p.target_profile = Some(id.clone()),
            }
        }
        None => p.target = fixed[i],
    }
    label(p, rivals)
}

/// What TARGET shows: its label, a rival's name.
pub fn label(p: &PlayConfig, rivals: &[Rival]) -> String {
    match (p.target, chosen(p, rivals)) {
        (Target::Rival, Some(r)) => format!("RIVAL {}", r.name),
        (Target::Rival, None) => "RIVAL (none)".into(),
        (t, _) => t.label().into(),
    }
}

/// The target's name for skins (a rival's name, else TARGET's label), and
/// the rival's when TARGET is one.
pub fn name(ctx: &Ctx) -> (String, Option<String>) {
    let p = &ctx.config.play;
    let rivals = rivals(ctx);
    match chosen(p, &rivals) {
        Some(r) => (r.name.clone(), Some(r.name.clone())),
        None => (label(p, &rivals), None),
    }
}

/// The target EX score of `song`, of `max_ex`, with the personal best
/// `best`: a rival's as the IR has it, or as their profile's record has it
/// (0 when there is none).
pub fn score(ctx: &Ctx, song: &Song, max_ex: u32, best: u32) -> u32 {
    let p = &ctx.config.play;
    if p.target != Target::Rival {
        return p.target.score(max_ex, best);
    }
    let ex = match (&p.target_profile, &p.target_rival) {
        (Some(id), _) => ctx.rival_best_of(id, song).map(|b| b.ex_score),
        (None, Some(k)) => {
            let lamp = ctx
                .net
                .ir
                .lamp(k.provider, &k.player, &song.md5, &song.sha256);
            lamp.and_then(|l| l.ex)
        }
        (None, None) => None,
    };
    ex.unwrap_or(0).min(max_ex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibebms_ir::ProviderId;

    #[test]
    fn rivals_after_best() {
        let rival = |n: &str| Rival {
            key: RivalKey::Ir(PlayerKey {
                provider: ProviderId::BmsIr,
                player: n.into(),
            }),
            name: n.to_uppercase(),
        };
        let rivals = [rival("a"), rival("b")];
        let mut p = PlayConfig::default();
        assert_eq!(step(&mut p, &rivals, 1), "RIVAL A");
        assert_eq!(step(&mut p, &rivals, 1), "RIVAL B");
        assert_eq!(p.target_rival.as_ref().unwrap().player, "b");
        assert_eq!(step(&mut p, &rivals, 1), "RANK A-");
        assert_eq!(step(&mut p, &rivals, -1), "RIVAL B");
        // a rival no longer followed: the list goes on from the first
        assert_eq!(label(&p, &rivals[..1]), "RIVAL (none)");
        assert_eq!(step(&mut p, &rivals[..1], 1), "RANK A-");
        // without rivals: as before
        p.target = Target::Best;
        assert_eq!(step(&mut p, &[], 1), "RANK A-");
        assert_eq!(step(&mut p, &[], -1), "BEST");
    }
}
