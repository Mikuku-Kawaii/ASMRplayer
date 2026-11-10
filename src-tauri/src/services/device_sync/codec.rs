use crate::domain::AppResult;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
fn error(_: serde_json::Error) -> crate::domain::AppError {
    super::fail("同步数据格式无效")
}
pub fn from_str<T: DeserializeOwned>(v: &str) -> AppResult<T> {
    serde_json::from_str(v).map_err(error)
}
pub fn from_value<T: DeserializeOwned>(v: Value) -> AppResult<T> {
    serde_json::from_value(v).map_err(error)
}
pub fn to_string<T: Serialize + ?Sized>(v: &T) -> AppResult<String> {
    serde_json::to_string(v).map_err(error)
}
pub fn to_value<T: Serialize>(v: T) -> AppResult<Value> {
    serde_json::to_value(v).map_err(error)
}
