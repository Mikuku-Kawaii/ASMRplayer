use crate::{db::Database, domain::{AppError, AppResult, ErrorCode}};
use serde::{Deserialize, Serialize};

const KEY: &str = "file_preferences_v1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effects { #[default] Any, With, Without }

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format { #[default] Any, Mp3, Wav }

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilePreferences { pub effects: Effects, pub format: Format }

pub fn read(db: &Database) -> AppResult<FilePreferences> {
    match db.get_setting(KEY)? {
        None => Ok(FilePreferences::default()),
        Some(raw) => serde_json::from_str(&raw).map_err(|_| AppError::new(ErrorCode::InvalidInput, "文件偏好无法读取")),
    }
}

pub fn save(db: &Database, value: FilePreferences) -> AppResult<FilePreferences> {
    let raw = serde_json::to_string(&value).map_err(|_| AppError::new(ErrorCode::InternalError, "文件偏好无法保存"))?;
    db.set_setting(KEY, &raw)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_preferences_survive_reopening_and_reject_unknown_choices() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data"); let cache = dir.path().join("cache");
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        assert_eq!(read(&db).unwrap(), FilePreferences::default());
        let selected = FilePreferences { effects: Effects::Without, format: Format::Mp3 };
        save(&db, selected.clone()).unwrap();
        drop(db);
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        assert_eq!(read(&db).unwrap(), selected);
        assert!(serde_json::from_str::<FilePreferences>(r#"{"effects":"invalid","format":"wav"}"#).is_err());
        assert_eq!(read(&db).unwrap(), selected);
    }
}
