//! What kind of skin a file is, and the modes its objects take: how they
//! move between keyframes, fit an image, align and run over.

/// The format of a skin file, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinFormat {
    /// LR2 `.lr2skin`, or a `.csv` chosen as the skin itself.
    Lr2,
    /// beatoraja JSON or Lua.
    Beatoraja,
    /// LR2 sound set `.lr2ss`.
    SoundSet,
}

impl SkinFormat {
    pub fn of(path: &std::path::Path) -> Option<SkinFormat> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        Some(match ext.as_str() {
            "lr2skin" | "csv" => SkinFormat::Lr2,
            "json" | "luaskin" => SkinFormat::Beatoraja,
            "lr2ss" => SkinFormat::SoundSet,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinType {
    Play7,
    Play5,
    Play14,
    Play10,
    Play9,
    /// vibeBMS's own types 104 / 106 / 108: 4 / 6 / 8 keys (Qwilight-style
    /// charts). Neither beatoraja nor LR2 has a type for them, so only the
    /// built-in default skins are of these.
    Play4,
    Play6,
    Play8,
    /// LR2 types 12 / 13 (beatoraja's "7/5KEYS BATTLE"): a double play
    /// layout for two sides of a battle.
    Battle7,
    Battle5,
    Select,
    Decide,
    Result,
    /// beatoraja / LR2 type 8: the key config screen.
    KeyConfig,
    /// Type 9: the skin select (customize) screen.
    SkinSelect,
    /// Type 10: a system sound set (LR2 `.lr2ss`), chosen like a skin.
    SoundSet,
    CourseResult,
    Other(i32),
}

impl SkinType {
    pub fn from_id(i: i32) -> SkinType {
        match i {
            0 => SkinType::Play7,
            1 => SkinType::Play5,
            2 => SkinType::Play14,
            3 => SkinType::Play10,
            4 => SkinType::Play9,
            5 => SkinType::Select,
            6 => SkinType::Decide,
            7 => SkinType::Result,
            8 => SkinType::KeyConfig,
            9 => SkinType::SkinSelect,
            10 => SkinType::SoundSet,
            12 => SkinType::Battle7,
            13 => SkinType::Battle5,
            15 => SkinType::CourseResult,
            104 => SkinType::Play4,
            106 => SkinType::Play6,
            108 => SkinType::Play8,
            n => SkinType::Other(n),
        }
    }

    /// The type's number (LR2 `#INFORMATION`, beatoraja `type`).
    pub fn id(self) -> i32 {
        match self {
            SkinType::Play7 => 0,
            SkinType::Play5 => 1,
            SkinType::Play14 => 2,
            SkinType::Play10 => 3,
            SkinType::Play9 => 4,
            SkinType::Select => 5,
            SkinType::Decide => 6,
            SkinType::Result => 7,
            SkinType::KeyConfig => 8,
            SkinType::SkinSelect => 9,
            SkinType::SoundSet => 10,
            SkinType::Battle7 => 12,
            SkinType::Battle5 => 13,
            SkinType::CourseResult => 15,
            SkinType::Play4 => 104,
            SkinType::Play6 => 106,
            SkinType::Play8 => 108,
            SkinType::Other(n) => n,
        }
    }

    pub fn mode(self) -> Option<vibebms_core::PlayMode> {
        use vibebms_core::PlayMode::*;
        Some(match self {
            SkinType::Play7 => Beat7K,
            SkinType::Play5 => Beat5K,
            SkinType::Play14 | SkinType::Battle7 => Beat14K,
            SkinType::Play10 | SkinType::Battle5 => Beat10K,
            SkinType::Play9 => Popn9K,
            SkinType::Play4 => Key4,
            SkinType::Play6 => Key6,
            SkinType::Play8 => Key8,
            _ => return None,
        })
    }

    /// Key of non-play skins in `config.skins` (play skins use the mode label).
    pub fn config_key(self) -> Option<&'static str> {
        Some(match self {
            SkinType::Select => "SELECT",
            SkinType::Decide => "DECIDE",
            SkinType::Result => "RESULT",
            SkinType::CourseResult => "COURSE_RESULT",
            SkinType::KeyConfig => "KEY_CONFIG",
            SkinType::SkinSelect => "SKIN_SELECT",
            SkinType::SoundSet => "SOUND_SET",
            SkinType::Battle7 => "7KEYS BATTLE",
            SkinType::Battle5 => "5KEYS BATTLE",
            _ => return None,
        })
    }

    /// The `config.skins` key of every type (play skins by mode label).
    pub fn key(self) -> Option<&'static str> {
        if self.is_battle() {
            return self.config_key();
        }
        self.mode().map(|m| m.label()).or_else(|| self.config_key())
    }

    /// A battle play skin (types 12 / 13).
    pub fn is_battle(self) -> bool {
        matches!(self, SkinType::Battle7 | SkinType::Battle5)
    }

    /// The battle type for a double play type and back (what a battle
    /// slot accepts besides its own type).
    pub fn battle_pair(self) -> Option<SkinType> {
        Some(match self {
            SkinType::Battle7 => SkinType::Play14,
            SkinType::Battle5 => SkinType::Play10,
            SkinType::Play14 => SkinType::Battle7,
            SkinType::Play10 => SkinType::Battle5,
            _ => return None,
        })
    }
}

/// beatoraja skin select settings (`skinSelect` of a JSON / Lua skin).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkinSelectInfo {
    /// The skin type shown first (`defaultCategory`).
    pub default_category: i32,
    /// Option rows the skin shows (`customPropertyCount`; 0: as many as
    /// it has buttons 220-229 for).
    pub custom_property_count: usize,
}

/// How a destination moves from one keyframe to the next (`acc`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Acc {
    #[default]
    Linear,
    /// Slow first (1).
    Accelerate,
    /// Fast first (2).
    Decelerate,
    /// Each keyframe as it is until the next (3).
    Hold,
}

impl Acc {
    pub fn of(v: i32) -> Acc {
        match v {
            1 => Acc::Accelerate,
            2 => Acc::Decelerate,
            3 => Acc::Hold,
            _ => Acc::Linear,
        }
    }
}

/// How an image fits its destination (LR2 `#STRETCH`); the image keeps
/// its aspect ratio but in `Full`, and is centred.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stretch {
    /// Stretched over the destination (0).
    #[default]
    Full,
    /// As large as fits inside (1).
    Fit,
    /// As small as covers it (2), cut off at it (3).
    Cover,
    CoverCut,
    /// As wide as it (4), cut off at it (5).
    Width,
    WidthCut,
    /// As high as it (6), cut off at it (7).
    Height,
    HeightCut,
    /// As `Fit`, but never larger than the image (8).
    FitDown,
    /// The image's own size (9), cut off at it (10).
    Own,
    OwnCut,
}

impl Stretch {
    pub fn of(v: i32) -> Stretch {
        match v {
            1 => Stretch::Fit,
            2 => Stretch::Cover,
            3 => Stretch::CoverCut,
            4 => Stretch::Width,
            5 => Stretch::WidthCut,
            6 => Stretch::Height,
            7 => Stretch::HeightCut,
            8 => Stretch::FitDown,
            9 => Stretch::Own,
            10 => Stretch::OwnCut,
            _ => Stretch::Full,
        }
    }
}

/// Where a text or a number sits across its destination: a number's
/// digits fill its places from the left, the right or the middle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    /// A text's `align`: 0 left, 1 centre, 2 right (others the nearest).
    pub fn of_text(v: i32) -> Align {
        match v {
            1 => Align::Center,
            2.. => Align::Right,
            _ => Align::Left,
        }
    }

    /// A number's `align`: 0 right (the blank places in front of its
    /// digits), 1 left (the blanks after them), 2 centred (others right).
    pub fn of_number(v: i32) -> Align {
        match v {
            1 => Align::Left,
            2 => Align::Center,
            _ => Align::Right,
        }
    }

    /// The share of the width that is left of the anchor: 0, ½, 1.
    pub fn share(self) -> f32 {
        match self {
            Align::Left => 0.0,
            Align::Center => 0.5,
            Align::Right => 1.0,
        }
    }
}

/// What a text wider than its destination does (`overflow`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overflow {
    /// Runs over it (0: LR2, beatoraja's default).
    #[default]
    Spill,
    /// Shrinks to fit (1).
    Shrink,
    /// Is cut off (2).
    Cut,
    /// vibeBMS (3): is cut off, and scrolls to its end and back while it
    /// is the current one (a marquee).
    Marquee,
}

impl Overflow {
    pub fn of(v: i32) -> Overflow {
        match v {
            1 => Overflow::Shrink,
            2 => Overflow::Cut,
            3 => Overflow::Marquee,
            _ => Overflow::Spill,
        }
    }
}

/// The way a bar graph fills or a slider moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Right,
    Down,
    Left,
}

impl Dir {
    /// A slider's direction: 0 up, 1 right, 2 down, others left.
    pub fn of_slider(v: i32) -> Dir {
        match v {
            0 => Dir::Up,
            1 => Dir::Right,
            2 => Dir::Down,
            _ => Dir::Left,
        }
    }

    /// A bar graph's direction: 1 fills upwards, others to the right.
    pub fn of_bar(v: i32) -> Dir {
        match v {
            1 => Dir::Up,
            _ => Dir::Right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A code below or above those there are: what it always meant.
    #[test]
    fn codes_out_of_range() {
        for v in [-1, i32::MIN] {
            assert_eq!(Acc::of(v), Acc::Linear);
            assert_eq!(Stretch::of(v), Stretch::Full);
            assert_eq!(Align::of_text(v), Align::Left);
            assert_eq!(Align::of_number(v), Align::Right);
            assert_eq!(Overflow::of(v), Overflow::Spill);
            assert_eq!(Dir::of_slider(v), Dir::Left);
            assert_eq!(Dir::of_bar(v), Dir::Right);
        }
        assert_eq!(Acc::of(4), Acc::Linear);
        assert_eq!(Stretch::of(11), Stretch::Full);
        assert_eq!(Align::of_text(3), Align::Right);
        assert_eq!(Align::of_text(i32::MAX), Align::Right);
        assert_eq!(Align::of_number(3), Align::Right);
        assert_eq!(Overflow::of(4), Overflow::Spill);
        assert_eq!(Dir::of_slider(4), Dir::Left);
        assert_eq!(Dir::of_bar(2), Dir::Right);
    }
}
