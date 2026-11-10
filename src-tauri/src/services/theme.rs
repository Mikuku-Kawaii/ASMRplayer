use crate::{db::Database, domain::{AppError, AppResult, ErrorCode}};
use serde::{Deserialize, Serialize};

const KEY: &str = "theme_preset";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    InkBlue,
    InkBlueLight,
    InkBlueSystem,
    Berry,
    #[default]
    Wisteria,
    Amber,
}

pub fn read(db: &Database) -> AppResult<ThemePreset> {
    Ok(db.get_setting(KEY)?.and_then(|value| serde_json::from_str(&value).ok()).unwrap_or_default())
}

pub fn save(db: &Database, preset: ThemePreset) -> AppResult<ThemePreset> {
    let value = serde_json::to_string(&preset)
        .map_err(|error| AppError::new(ErrorCode::InternalError, error.to_string()))?;
    db.set_setting(KEY, &value)?;
    Ok(preset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_wisteria_and_persists_all_presets() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let cache = dir.path().join("cache");
        let mut db = Database::open_and_migrate(&data, &cache).unwrap();
        assert_eq!(read(&db).unwrap(), ThemePreset::Wisteria);
        for preset in [ThemePreset::Berry, ThemePreset::Amber, ThemePreset::Wisteria, ThemePreset::InkBlue, ThemePreset::InkBlueLight, ThemePreset::InkBlueSystem] {
            save(&db, preset).unwrap();
            drop(db);
            db = Database::open_and_migrate(&data, &cache).unwrap();
            assert_eq!(read(&db).unwrap(), preset);
        }
        db.set_setting(KEY, "\"unknown\"").unwrap();
        assert_eq!(read(&db).unwrap(), ThemePreset::Wisteria);
        assert!(serde_json::from_str::<ThemePreset>("\"unknown\"").is_err());
    }
}
