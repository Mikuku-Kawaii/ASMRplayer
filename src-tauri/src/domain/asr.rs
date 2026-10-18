//! Persisted evidence from the recognizer, never inferred by the translation model.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SegmentEvidence {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub avg_logprob: Option<f64>,
    pub no_speech_prob: Option<f64>,
    pub compression_ratio: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SpeechActivityEvidence {
    pub model: String,
    pub max_probability: f64,
    pub mean_probability: f64,
    pub frames: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RecognitionEvidence {
    pub text: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub tokens: Vec<String>,
    #[serde(default)]
    pub timestamps: Vec<f64>,
    #[serde(default)]
    pub segments: Vec<SegmentEvidence>,
    #[serde(default, alias = "lang")]
    pub detected_language: Option<String>,
    #[serde(default)]
    pub config: serde_json::Value,
    /// Optional unfiltered PCM diagnostic, never a stand-alone deletion gate.
    #[serde(default)]
    pub speech_activity: Option<SpeechActivityEvidence>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioActivity {
    pub rms: f64,
    pub peak: f64,
    pub nonzero_fraction: f64,
    pub active_frame_fraction: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrAttempt {
    pub start_ms: i64,
    pub end_ms: i64,
    pub prompt: String,
    pub elapsed_ms: u64,
    pub audio: AudioActivity,
    pub result: RecognitionEvidence,
    pub signals: Vec<String>,
    #[serde(default)]
    pub repetition_ratio: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrRecord {
    #[serde(default)]
    pub dialogue_review_history: Vec<DialogueReview>,
    #[serde(default)]
    pub dialogue_review: Option<DialogueReview>,
    pub model: String,
    pub language: String,
    pub attempts: Vec<AsrAttempt>,
    pub selected_attempt: usize,
    pub pending_audio_review: bool,
    pub confirmed_no_speech: bool,
    pub human_confirmed: bool,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DialogueDecision { Retain, Review, NonDialogue, Unavailable }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogueObservation {
    pub model: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub context: bool,
    pub elapsed_ms: u64,
    pub result: RecognitionEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogueReview {
    #[serde(default)]
    pub revision: u8,
    #[serde(default)]
    pub context_ms: i64,
    pub decision: DialogueDecision,
    pub note: String,
    pub observations: Vec<DialogueObservation>,
}

/// Remove standalone sound captions without rewriting spoken sentences.
pub fn dialogue_only(text: &str) -> String {
    fn sound(text: &str) -> bool {
        let label = text.trim();
        let bracketed = [('[', ']'), ('【', '】'), ('(', ')'), ('（', '）')].iter()
            .any(|(start, end)| label.starts_with(*start) && label.ends_with(*end));
        let mut remaining = text.trim_matches(|c: char| c.is_whitespace()
            || "。．！？，、；：!?.,;:…〜～ー~♪♡♥★☆（）()[]【】「」『』".contains(c));
        if remaining.is_empty() { return true; }
        if bracketed && ["呼吸", "呼吸声", "吐息", "笑い声", "笑声", "舐め音", "リップ音", "舔舐声", "摩擦声", "咳嗽声", "拍手", "音楽", "音乐", "BGM", "breathing", "laughter", "coughing", "music"].contains(&remaining) { return true; }
        if remaining.chars().count() >= 3 && ['ヘ', 'へ', 'フ', 'ふ'].iter().any(|c| remaining.chars().all(|value| value == *c)) { return true; }
        const UNITS: &[&str] = &["ジュル", "じゅる", "チュッ", "ちゅっ", "チュ", "ちゅ", "ペロ", "ぺろ", "ゴク", "ごく", "フッ", "ふっ", "ハァ", "はぁ", "ハア", "はあ", "ハッ", "はっ", "フゥ", "ふぅ", "ふう", "フフ", "ふふ", "ハハ", "はは", "へへ", "ヘヘ", "こほん", "はくしょん", "しくしく", "啾噜", "咕噜", "咳咳", "呵呵", "哈哈", "呼呼", "呼", "阿嚏", "呜呜"];
        while !remaining.is_empty() {
            let Some(unit) = UNITS.iter().find(|unit| remaining.starts_with(**unit)) else { return false; };
            remaining = remaining[unit.len()..].trim_start_matches(|c| "ー〜～~っッ".contains(c));
        }
        true
    }
    text.lines().filter_map(|line| {
        if sound(line) { return None; }
        let words: Vec<_> = line.split_whitespace().filter(|word| !sound(word)).collect();
        (!words.is_empty()).then(|| words.join(" "))
    }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod dialogue_tests {
    use super::dialogue_only;
    #[test]
    fn removes_sound_captions_but_preserves_spoken_words_and_long_vowels() {
        for sound in ["ジュルジュル", "ペロペロ", "チュッチュッ", "フッフッフッ", "はぁ～", "ふふふふ", "呼呼", "[breathing]", "（舐め音）", "【笑い声】", "ヘヘヘヘヘヘ"] {
            assert!(dialogue_only(sound).is_empty(), "{sound}");
        }
        for dialogue in ["ごめん", "ごめんごめんごめん", "おやすーーーーみ", "うん", "はい", "ああああ", "呼吸", "音楽", "ペロペロしてあげる", "ふふって笑った", "はは母の話です"] {
            assert_eq!(dialogue_only(dialogue), dialogue);
        }
        assert_eq!(dialogue_only("こんにちは フッフッフッ またね\nジュルジュル\nごめん"), "こんにちは またね\nごめん");
    }
}

// Caption repeated nonverbal sounds separately without deleting adjacent dialogue.
/// Presentation only: never alter stored ASR, IDs or timing. Boundary isolation
/// avoids treating syllables inside ordinary words or sustained vowels as sounds.
pub fn display(text: &str) -> String {
    let units: Vec<Vec<char>> = [
        "ジュル",
        "じゅる",
        "チュッ",
        "ちゅっ",
        "チュ",
        "ちゅ",
        "ペロ",
        "ぺろ",
        "ゴク",
        "ごく",
        "フッ",
        "ふっ",
        "ハァ",
        "はぁ",
        "ハッ",
        "はっ",
        "ヘ",
        "へ",
        "フ",
        "ふ",
    ]
    .iter()
    .map(|u| u.chars().collect())
    .collect();
    let chars: Vec<_> = text.chars().collect();
    let boundary = |c: char| {
        c.is_whitespace()
            || c.is_ascii_punctuation()
            || "。．！？，、；：「」『』【】（）…〜～・".contains(c)
    };
    let mut output = String::new();
    let mut pos = 0;
    while pos < chars.len() {
        let mut end = pos;
        let mut sounds = Vec::new();
        if pos == 0 || boundary(chars[pos - 1]) {
            loop {
                let found = units.iter().find_map(|unit| {
                    let mut count = 0;
                    while chars.get(end + count * unit.len()..end + (count + 1) * unit.len())
                        == Some(unit.as_slice())
                    {
                        count += 1;
                    }
                    (count >= if unit.len() == 1 { 6 } else { 1 }).then(|| {
                        (
                            end + count * unit.len(),
                            unit.iter().collect::<String>().repeat(count.min(3)),
                        )
                    })
                });
                let Some((next, sound)) = found else {
                    break;
                };
                sounds.push(sound);
                end = next;
            }
        }
        if end > pos && (end == chars.len() || boundary(chars[end])) {
            if !output.trim().is_empty() {
                output = output.trim_end().to_owned();
                output.push('\n');
            }
            output.push_str(&sounds.join("\n"));
            if end < chars.len() {
                output.push('\n');
            }
            pos = end;
        } else {
            if !(output.ends_with('\n') && chars[pos].is_whitespace()) {
                output.push(chars[pos]);
            }
            pos += 1;
        }
    }
    output.trim().to_owned()
}
