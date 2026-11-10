use crate::domain::{AppError, AppResult, ErrorCode};
use serde::{Deserialize, Serialize};

pub const SETTINGS_KEY: &str = "immersion_settings";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ImmersionSettings {
    pub reverse_stereo: bool,
    pub gapless_playback: bool,
    pub fade_enabled: bool,
    pub fade_duration_ms: u64,
}
impl Default for ImmersionSettings {
    fn default() -> Self {
        Self {
            reverse_stereo: false,
            gapless_playback: true,
            fade_enabled: false,
            fade_duration_ms: 250,
        }
    }
}
impl ImmersionSettings {
    pub fn validate(&self) -> AppResult<()> {
        if !(100..=1500).contains(&self.fade_duration_ms) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "过渡时长须为 100–1500 毫秒",
            ));
        }
        Ok(())
    }
}
pub fn output_graph(base: &str, reverse: bool) -> String {
    if !reverse {
        return base.into();
    }
    let swap = "lavfi=[aformat=channel_layouts=stereo,pan=stereo|c0=c1|c1=c0]";
    if base.is_empty() {
        swap.into()
    } else {
        format!("{base},{swap}")
    }
}
pub fn fade_gain(from: f64, to: f64, progress: f64) -> f64 {
    let t = progress.clamp(0.0, 1.0);
    from + (to - from) * (t * t * (3.0 - 2.0 * t))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn immersion_defaults_preserve_existing_output_and_validation() {
        let settings: ImmersionSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, ImmersionSettings::default());
        assert!(settings.gapless_playback && !settings.fade_enabled && !settings.reverse_stereo);
        assert!(ImmersionSettings {
            fade_duration_ms: 0,
            ..settings
        }
        .validate()
        .is_err());
        assert_eq!(output_graph("", false), "");
        assert_eq!(
            output_graph("lavfi=[volume=0.5]", false),
            "lavfi=[volume=0.5]"
        );
        assert_eq!(
            output_graph("", true),
            "lavfi=[aformat=channel_layouts=stereo,pan=stereo|c0=c1|c1=c0]"
        );
        assert_eq!(output_graph("lavfi=[alimiter=level=0]",true),"lavfi=[alimiter=level=0],lavfi=[aformat=channel_layouts=stereo,pan=stereo|c0=c1|c1=c0]");
    }
    #[test]
    fn immersion_fade_is_linked_bounded_monotonic_and_has_smooth_endpoints() {
        let samples = (0..=100)
            .map(|i| fade_gain(1.0, 0.0, i as f64 / 100.0))
            .collect::<Vec<_>>();
        assert_eq!(samples[0], 1.0);
        assert_eq!(samples[100], 0.0);
        assert!(samples.windows(2).all(|w| w[1] <= w[0]));
        assert!(samples[0] - samples[1] < 0.001);
        assert!(samples[99] < 0.001);
        for i in 0..=100 {
            assert!((fade_gain(0.0, 1.0, i as f64 / 100.0) + samples[i] - 1.0).abs() < 1e-12);
        }
    }
}
