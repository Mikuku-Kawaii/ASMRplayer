use crate::{db::Database, domain::{AppError, AppResult, ErrorCode}};

const KEY: &str = "interface_scale";

fn validate(scale: f64) -> AppResult<f64> {
    if !scale.is_finite() || !(0.5..=1.5).contains(&scale) {
        return Err(AppError::new(ErrorCode::InvalidInput, "界面缩放须在 50% 至 150% 之间"));
    }
    Ok((scale * 10.0).round() / 10.0)
}

pub fn read(db: &Database) -> AppResult<f64> {
    Ok(db.get_setting(KEY)?.and_then(|s| s.parse().ok())
        .and_then(|scale| validate(scale).ok()).unwrap_or(1.0))
}

pub fn save(db: &Database, scale: f64, apply: impl Fn(f64) -> AppResult<()>) -> AppResult<f64> {
    let scale = validate(scale)?;
    let previous = read(db)?;
    apply(scale)?;
    if let Err(error) = db.set_setting(KEY, &scale.to_string()) {
        let _ = apply(previous);
        return Err(error);
    }
    Ok(scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_is_validated_durable_and_failed_apply_keeps_previous_preference() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let cache = dir.path().join("cache");
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        assert_eq!(read(&db).unwrap(), 1.0);
        for scale in [f64::NAN, f64::INFINITY, 0.49, 1.51] {
            assert!(save(&db, scale, |_| panic!("invalid value must not reach window")).is_err());
        }
        assert_eq!(save(&db, 0.8, |value| { assert_eq!(value, 0.8); Ok(()) }).unwrap(), 0.8);
        assert!(save(&db, 1.2, |_| Err(AppError::new(ErrorCode::InternalError, "window unavailable"))).is_err());
        assert_eq!(read(&db).unwrap(), 0.8);
        drop(db);
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        assert_eq!(read(&db).unwrap(), 0.8);
        save(&db, 1.0, |_| Ok(())).unwrap();
        assert_eq!(read(&db).unwrap(), 1.0);
        db.set_setting(KEY, "NaN").unwrap();
        assert_eq!(read(&db).unwrap(), 1.0);
    }
}
