//! Shared subtitle prefetch and exact nonverbal translation helpers.
pub use crate::domain::asr::RecognitionEvidence as RecognitionResult;
pub const CHUNK_MS: i64 = 20_000;
pub fn is_non_speech_word(text: &str) -> bool {
    non_speech_translation(text).is_some()
}
pub fn non_speech_translation(text: &str) -> Option<&'static str> {
    match text.trim() {
        "呼" | "ふう" => Some("呼"),
        "呵呵" | "ふふ" => Some("呵呵"),
        "咳咳" | "こほん" => Some("咳咳"),
        "呜呜" | "しくしく" => Some("呜呜"),
        "阿嚏" | "はくしょん" => Some("阿嚏"),
        "ジュル" | "じゅる" => Some("啾噜"),
        "ジュルジュル" | "じゅるじゅる" => Some("啾噜啾噜"),
        _ => None,
    }
}

pub fn prefetch_chunks(
    position_ms: i64,
    target_seconds: u32,
    total: usize,
) -> std::ops::Range<usize> {
    let start = (position_ms.max(0) / CHUNK_MS) as usize;
    let end =
        ((position_ms.max(0) + target_seconds as i64 * 1000 + CHUNK_MS - 1) / CHUNK_MS) as usize;
    start.min(total)..end.min(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t1513_prefetch_target_moves_with_seek_and_stops_at_eof() {
        assert_eq!(prefetch_chunks(0, 15, 10), 0..1);
        assert_eq!(prefetch_chunks(6000, 10, 10), 0..1);
        assert_eq!(prefetch_chunks(6000, 15, 10), 0..2);
        assert_eq!(prefetch_chunks(65_000, 20, 10), 3..5);
        assert_eq!(prefetch_chunks(196_000, 15, 10), 9..10);
        assert_eq!(prefetch_chunks(200_000, 15, 10), 10..10);
        assert_eq!(prefetch_chunks(0, 15, 0), 0..0);
    }
}
