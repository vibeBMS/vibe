//! What kind of skin a file is.

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
