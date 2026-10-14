//! Application-window bindings only. Windows SMTC media keys remain system-managed.
use super::Database;
use crate::domain::{AppError, AppResult, ErrorCode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub type ShortcutBindings = BTreeMap<String, Option<String>>;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Command { id: String, default_binding: String }
#[derive(Serialize, Deserialize)]
struct StoredBindings { version: u8, bindings: ShortcutBindings }

fn invalid(message: &str) -> AppError { AppError::new(ErrorCode::InvalidInput, message) }
pub fn default_bindings() -> ShortcutBindings {
    serde_json::from_str::<Vec<Command>>(include_str!("../../../src/lib/commandCatalog.json"))
        .expect("built-in command catalog").into_iter().map(|command| (command.id, Some(command.default_binding))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn shortcuts_defaults_validation_conflicts_and_reserved_combinations() {
        assert!(validated_bindings(default_bindings()).is_ok());
        for value in ["KeyA","Ctrl+KeyC","Escape","Enter","MediaPlayPause","Ctrl+Ctrl+KeyG","Shift+Ctrl+KeyG","Alt+KeyG","Meta+KeyG","F5"] {
            assert!(validate_binding(value).is_err(),"{value}");
        }
        for value in ["Ctrl+Shift+KeyG","Ctrl+Alt+KeyG","F8","Ctrl+F5","Space","ArrowLeft"] { assert!(validate_binding(value).is_ok(),"{value}"); }
        let mut conflicting=default_bindings(); conflicting.insert("play_pause".into(),Some("Ctrl+KeyK".into()));
        assert!(validated_bindings(conflicting).is_err());
        assert!(validated_bindings(BTreeMap::from([("unknown".into(),None)])).is_err());
    }

    #[test]
    fn shortcuts_persist_disabled_and_custom_bindings_and_keep_previous_on_failure() {
        let dir=tempdir().unwrap();let data=dir.path().join("d");let cache=dir.path().join("c");
        let db=Database::open_and_migrate(&data,&cache).unwrap();
        let mut custom=default_bindings();custom.insert("play_pause".into(),Some("Ctrl+KeyG".into()));custom.insert("next".into(),None);
        db.save_shortcut_bindings(custom.clone()).unwrap();drop(db);
        let db=Database::open_and_migrate(&data,&cache).unwrap();assert_eq!(db.get_shortcut_bindings().unwrap(),custom);
        let mut bad=custom.clone();bad.insert("play_pause".into(),Some("Ctrl+KeyK".into()));
        assert!(db.save_shortcut_bindings(bad).is_err());assert_eq!(db.get_shortcut_bindings().unwrap(),custom);
        db.with_conn(|conn|{conn.execute_batch("CREATE TRIGGER block_shortcut_save BEFORE UPDATE ON app_settings WHEN NEW.key='shortcut_bindings_v1' BEGIN SELECT RAISE(ABORT,'injected failure'); END;")?;Ok(())}).unwrap();
        assert!(db.save_shortcut_bindings(default_bindings()).is_err());assert_eq!(db.get_shortcut_bindings().unwrap(),custom);
        db.with_conn(|conn|{conn.execute_batch("DROP TRIGGER block_shortcut_save;")?;Ok(())}).unwrap();
        db.save_shortcut_bindings(default_bindings()).unwrap();assert_eq!(db.get_shortcut_bindings().unwrap(),default_bindings());
    }

    #[test]
    fn shortcuts_corrupt_or_future_settings_report_errors_without_overwriting() {
        let dir=tempdir().unwrap();let db=Database::open_and_migrate(&dir.path().join("d"),&dir.path().join("c")).unwrap();
        db.set_setting("shortcut_bindings_v1","not json").unwrap();assert!(db.get_shortcut_bindings().is_err());
        assert_eq!(db.get_setting("shortcut_bindings_v1").unwrap().as_deref(),Some("not json"));
        db.set_setting("shortcut_bindings_v1",r#"{"version":2,"bindings":{}}"#).unwrap();assert!(db.get_shortcut_bindings().is_err());
        db.set_setting("shortcut_bindings_v1",r#"{"version":1,"bindings":{"play_pause":null}}"#).unwrap();
        let loaded=db.get_shortcut_bindings().unwrap();assert_eq!(loaded["play_pause"],None);assert_eq!(loaded["command_search"],Some("Ctrl+KeyK".into()));
    }
}
fn validate_binding(value: &str) -> AppResult<()> {
    const RESERVED: &[&str] = &["Ctrl+KeyW","Ctrl+KeyQ","Ctrl+KeyR","Ctrl+KeyL","Ctrl+KeyN","Ctrl+KeyT","Ctrl+KeyP","Ctrl+KeyO","Ctrl+KeyS","Ctrl+KeyF","Ctrl+KeyC","Ctrl+KeyV","Ctrl+KeyX","Ctrl+KeyZ","Ctrl+KeyA","Ctrl+KeyY","Ctrl+Shift+KeyR","Ctrl+Shift+KeyT","Ctrl+Shift+KeyN","Ctrl+Shift+KeyI","Ctrl+Shift+Delete","Alt+F4","Ctrl+Alt+Delete"];
    if RESERVED.contains(&value) { return Err(invalid("该组合由系统或编辑操作保留，请更换快捷键")); }
    let mut parts: Vec<_> = value.split('+').collect();
    let code = parts.pop().unwrap_or("");
    let modifiers: Vec<_> = ["Ctrl","Alt","Shift"].into_iter().filter(|key| parts.contains(key)).collect();
    if parts != modifiers { return Err(invalid("快捷键修饰键无效")); }
    let letter = code.strip_prefix("Key").is_some_and(|v| v.len() == 1 && v.as_bytes()[0].is_ascii_uppercase());
    let digit = code.strip_prefix("Digit").is_some_and(|v| v.len() == 1 && v.as_bytes()[0].is_ascii_digit());
    let function = (1..=10).any(|n| code == format!("F{n}"));
    let plain = ["Space","ArrowLeft","ArrowRight","ArrowUp","ArrowDown"].contains(&code) || function;
    if !(plain || letter || digit || ["Comma","Period"].contains(&code)) { return Err(invalid("不支持此按键，导航键与系统媒体键保留原有用途")); }
    if (!plain || parts.contains(&"Alt")) && !parts.contains(&"Ctrl") { return Err(invalid("此按键需搭配 Ctrl")); }
    if parts.is_empty() && ["F5","F10"].contains(&code) { return Err(invalid("此功能键保留给窗口操作，请搭配 Ctrl")); }
    Ok(())
}
pub fn validated_bindings(bindings: ShortcutBindings) -> AppResult<ShortcutBindings> {
    let mut result = default_bindings();
    for (id,value) in bindings {
        if !result.contains_key(&id) { return Err(invalid("存在未知命令，请重新加载快捷键")); }
        if let Some(value) = &value { validate_binding(value)?; }
        result.insert(id,value);
    }
    let mut seen = HashSet::new();
    for binding in result.values().flatten() {
        if !seen.insert(binding) { return Err(invalid("快捷键冲突，请修改或清除另一项后再保存")); }
    }
    Ok(result)
}
impl Database {
    pub fn get_shortcut_bindings(&self) -> AppResult<ShortcutBindings> {
        match self.get_setting("shortcut_bindings_v1")? {
            None => Ok(default_bindings()),
            Some(value) => {
                let stored: StoredBindings = serde_json::from_str(&value).map_err(|_| invalid("快捷键配置损坏，当前使用默认设置；可在设置中恢复默认"))?;
                if stored.version != 1 { return Err(invalid("快捷键配置版本不兼容，当前使用默认设置")); }
                validated_bindings(stored.bindings)
            }
        }
    }
    pub fn save_shortcut_bindings(&self, bindings: ShortcutBindings) -> AppResult<ShortcutBindings> {
        let bindings = validated_bindings(bindings)?;
        let value = serde_json::to_string(&StoredBindings { version:1, bindings:bindings.clone() })
            .map_err(|_| invalid("快捷键配置序列化失败"))?;
        self.set_setting("shortcut_bindings_v1", &value)?;
        Ok(bindings)
    }
}
