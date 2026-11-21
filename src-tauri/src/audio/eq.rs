//! Equalizer presets and mpv audio-filter string formatting.
//!
//! MPV-agnostic: this module owns the preset band tables and knows how to
//! render an mpv `af` filter string, but nothing about the running mpv
//! process. Applying the string to mpv lives in the audio engine
//! (`mpv_commands::apply_eq`).
//!
//! Filter choice: the shipped mpv exposes ffmpeg's 18-band `superequalizer`
//! libavfilter, so that is what we emit. (An older 10-band graphic `equalizer`
//! af exists in some builds; the branch fell back to it when `superequalizer`
//! was absent. Here `superequalizer` is present, so the 10-band tables are kept
//! only for the UI and are mapped up to 18 bands for the filter.)

/// Superequalizer band center frequencies in Hz (ffmpeg `superequalizer`,
/// `1b`..`18b`). Kept in sync with `BAND_FREQUENCIES_18` in `Equalizer.svelte`
/// and the `bands_18` doc below.
pub const SUPEREQ_FREQS: [f64; 18] = [
    65.0, 92.0, 131.0, 185.0, 262.0, 370.0, 523.0, 740.0, 1047.0, 1480.0, 2093.0, 2960.0, 4186.0,
    5920.0, 8372.0, 11840.0, 16744.0, 20000.0,
];

/// Anchor map from a 10-band UI index to the nearest 18-band superequalizer
/// index. Bands between anchors are interpolated (see `map_10_to_18_bands`).
const ANCHORS: [usize; 10] = [0, 1, 2, 3, 5, 7, 9, 11, 13, 16];

/// Equalizer preset configurations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqPreset {
    Flat,
    Rock,
    Pop,
    Classical,
    Electronic,
    Jazz,
    HipHop,
    Acoustic,
    BassBoost,
    TrebleBoost,
    Custom,
}

impl EqPreset {
    /// Get the 10-band gain values for this preset.
    /// Values are in dB, limited to ±3dB to prevent distortion and artifacts.
    /// Frequencies: 31, 63, 125, 250, 500, 1k, 2k, 4k, 8k, 16k Hz
    pub fn bands_10(&self) -> [i32; 10] {
        match self {
            EqPreset::Flat => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            // Rock: V-curve - emphasized bass and treble, subtle mid dip
            EqPreset::Rock => [3, 3, 2, 1, -1, -2, 0, 2, 2, 2],
            // Pop: gentle bass boost and presence peak for vocal clarity
            EqPreset::Pop => [2, 2, 1, 1, 0, 0, 2, 1, 1, 1],
            // Classical: subtle high end lift, gentle bass reduction
            EqPreset::Classical => [-1, -1, 0, 0, -1, 0, 1, 2, 2, 2],
            // Electronic: bass emphasis with gradual high frequency roll-up
            EqPreset::Electronic => [3, 3, 2, 1, -1, 0, 1, 2, 2, 1],
            // Jazz: warm low-mids, presence boost, gentle high roll-off
            EqPreset::Jazz => [1, 1, 1, 1, 0, 0, 1, 1, -1, -1],
            // Hip Hop: solid bass, scoop mids, crisp highs
            EqPreset::HipHop => [3, 3, 2, 1, -1, -2, 1, 2, 3, 2],
            // Acoustic: natural balanced curve with slight presence
            EqPreset::Acoustic => [1, 1, 1, 0, 0, 0, 1, 1, 1, 1],
            // Bass Boost: moderate low frequency emphasis (not extreme)
            EqPreset::BassBoost => [3, 3, 3, 2, 1, 0, 0, 0, 0, 0],
            // Treble Boost: gentle high frequency presence
            EqPreset::TrebleBoost => [0, 0, 0, 0, 0, 0, 1, 2, 3, 3],
            EqPreset::Custom => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        }
    }

    /// Get the 18-band gain values for this preset.
    /// Values are in dB, limited to ±3dB to prevent distortion and artifacts.
    /// Superequalizer bands: 1b=65Hz, 2b=92Hz, 3b=131Hz, 4b=185Hz, 5b=262Hz,
    /// 6b=370Hz, 7b=523Hz, 8b=740Hz, 9b=1047Hz, 10b=1480Hz, 11b=2093Hz,
    /// 12b=2960Hz, 13b=4186Hz, 14b=5920Hz, 15b=8372Hz, 16b=11840Hz,
    /// 17b=16744Hz, 18b=20000Hz
    pub fn bands_18(&self) -> [i32; 18] {
        match self {
            EqPreset::Flat => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            // Rock: V-curve - emphasized bass and treble, subtle mid dip
            EqPreset::Rock => [3, 3, 2, 1, 0, -1, -2, -1, 0, 1, 1, 2, 2, 2, 2, 2, 2, 2],
            // Pop: gentle bass boost and presence peak for vocal clarity
            EqPreset::Pop => [2, 2, 2, 1, 0, 0, -1, 0, 1, 1, 2, 2, 1, 1, 0, 1, 1, 1],
            // Classical: subtle high end lift, gentle bass reduction
            EqPreset::Classical => [-1, -1, 0, 0, 0, -1, 0, 0, 0, 1, 1, 1, 2, 2, 2, 2, 2, 2],
            // Electronic: bass emphasis with gradual high frequency roll-up
            EqPreset::Electronic => [3, 3, 2, 1, 0, -1, -1, 0, 0, 1, 1, 2, 2, 2, 2, 2, 1, 1],
            // Jazz: warm low-mids, presence boost, gentle high roll-off
            EqPreset::Jazz => [1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, -1, -1, -1, -1],
            // Hip Hop: solid bass, scoop mids, crisp highs
            EqPreset::HipHop => [3, 3, 2, 1, 0, -1, -2, -1, 0, 1, 1, 2, 2, 3, 3, 2, 2, 2],
            // Acoustic: natural balanced curve with slight presence
            EqPreset::Acoustic => [1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
            // Bass Boost: moderate low frequency emphasis (not extreme)
            EqPreset::BassBoost => [3, 3, 3, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            // Treble Boost: gentle high frequency presence
            EqPreset::TrebleBoost => [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 2, 2, 3, 3, 3, 3],
            EqPreset::Custom => [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        }
    }

    /// Get the preset name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            EqPreset::Flat => "Flat",
            EqPreset::Rock => "Rock",
            EqPreset::Pop => "Pop",
            EqPreset::Classical => "Classical",
            EqPreset::Electronic => "Electronic",
            EqPreset::Jazz => "Jazz",
            EqPreset::HipHop => "Hip Hop",
            EqPreset::Acoustic => "Acoustic",
            EqPreset::BassBoost => "Bass Boost",
            EqPreset::TrebleBoost => "Treble Boost",
            EqPreset::Custom => "Custom",
        }
    }

    /// Parse from string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "flat" => Some(EqPreset::Flat),
            "rock" => Some(EqPreset::Rock),
            "pop" => Some(EqPreset::Pop),
            "classical" => Some(EqPreset::Classical),
            "electronic" => Some(EqPreset::Electronic),
            "jazz" => Some(EqPreset::Jazz),
            "hip hop" | "hiphop" | "hip_hop" => Some(EqPreset::HipHop),
            "acoustic" => Some(EqPreset::Acoustic),
            "bass boost" | "bassboost" | "bass_boost" => Some(EqPreset::BassBoost),
            "treble boost" | "trebleboost" | "treble_boost" => Some(EqPreset::TrebleBoost),
            "custom" => Some(EqPreset::Custom),
            _ => None,
        }
    }

    /// Get all available (selectable) presets.
    pub fn all() -> &'static [EqPreset] {
        &[
            EqPreset::Flat,
            EqPreset::Rock,
            EqPreset::Pop,
            EqPreset::Classical,
            EqPreset::Electronic,
            EqPreset::Jazz,
            EqPreset::HipHop,
            EqPreset::Acoustic,
            EqPreset::BassBoost,
            EqPreset::TrebleBoost,
        ]
    }
}

/// Down-sample an 18-band curve back to the 10 UI bands by reading each UI
/// band's anchor (the inverse of [`map_10_to_18_bands`]'s anchor pinning). Used
/// to present a stored 18-band user profile in the 10-band view.
pub fn extract_10_from_18(bands_18: &[i32; 18]) -> [i32; 10] {
    let mut bands_10 = [0i32; 10];
    for (k, &anchor) in ANCHORS.iter().enumerate() {
        bands_10[k] = bands_18[anchor];
    }
    bands_10
}

/// Map 10-band UI values to the 18-band superequalizer format.
///
/// Each UI band is pinned to its nearest superequalizer band (the `ANCHORS`:
/// 31Hz->1b, 63Hz->2b, 125Hz->3b, 250Hz->4b, 500Hz->6b, 1kHz->8b, 2kHz->10b,
/// 4kHz->12b, 8kHz->14b, 16kHz->17b). Every band between two anchors is filled
/// in by log-frequency interpolation, and the final band (18b/20000Hz, above
/// the 16kHz anchor) holds the top UI value. This yields a smooth curve instead
/// of the comb-like response you get from leaving the in-between bands neutral.
pub fn map_10_to_18_bands(bands_10: &[i32; 10]) -> [i32; 18] {
    let mut bands_18 = [0i32; 18];

    // Pin each UI band to its anchor superequalizer band.
    for (k, &anchor) in ANCHORS.iter().enumerate() {
        bands_18[anchor] = bands_10[k];
    }

    // Fill every band between two anchors by log-frequency interpolation so the
    // response tracks perceived pitch instead of leaving comb-like gaps at 0.
    for pair in ANCHORS.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        let (g_lo, g_hi) = (bands_18[lo] as f64, bands_18[hi] as f64);
        let (f_lo, f_hi) = (SUPEREQ_FREQS[lo].log10(), SUPEREQ_FREQS[hi].log10());
        for i in (lo + 1)..hi {
            let t = (SUPEREQ_FREQS[i].log10() - f_lo) / (f_hi - f_lo);
            bands_18[i] = (g_lo + t * (g_hi - g_lo)).round() as i32;
        }
    }

    // The top anchor (16kHz -> 17b/16744Hz) sits below the final band
    // (18b/20000Hz), so hold the highest UI value out to the top.
    for slot in bands_18.iter_mut().skip(ANCHORS[9] + 1) {
        *slot = bands_10[9];
    }

    bands_18
}

/// Format 18-band values as an mpv `superequalizer` filter string.
///
/// FFmpeg's `superequalizer` takes 18 named band parameters (`1b`..`18b`) as
/// MULTIPLIERS where `1.0` is flat/neutral (`2.0` = +6dB, `0.5` = -6dB). Our
/// internal values are integer dB, so convert with `multiplier = 10^(dB/20)`.
/// Gains are clamped to ±20dB (matching the UI), i.e. the `[0.1, 10]`
/// multiplier range, comfortably inside the filter's accepted `[0, 20]`.
pub fn format_supereq_string(bands: [i32; 18]) -> String {
    let params: Vec<String> = bands
        .iter()
        .enumerate()
        .map(|(i, &g)| {
            let db = (g as f64).clamp(-20.0, 20.0);
            let multiplier = 10_f64.powf(db / 20.0);
            format!("{}b={:.3}", i + 1, multiplier)
        })
        .collect();
    format!("superequalizer={}", params.join(":"))
}

/// Build the mpv `af` filter string for an arbitrary band slice.
///
/// Accepts 18 bands (canonical) or fewer (missing bands treated as neutral),
/// so it can render straight from the engine's stored `Vec<i32>` EQ state.
pub fn filter_string(bands: &[i32]) -> String {
    let mut arr = [0i32; 18];
    for (slot, &value) in arr.iter_mut().zip(bands.iter()) {
        *slot = value;
    }
    format_supereq_string(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eq_preset_flat() {
        assert_eq!(EqPreset::Flat.bands_18(), [0i32; 18]);
        assert_eq!(EqPreset::Flat.bands_10(), [0i32; 10]);
    }

    #[test]
    fn test_eq_preset_rock() {
        let bands = EqPreset::Rock.bands_18();
        assert!(bands[0] > 0); // Bass
        assert!(bands[17] > 0); // Treble
    }

    #[test]
    fn test_eq_preset_to_string() {
        assert_eq!(EqPreset::Flat.as_str(), "Flat");
        assert_eq!(EqPreset::BassBoost.as_str(), "Bass Boost");
    }

    #[test]
    fn test_eq_preset_from_str() {
        assert_eq!(EqPreset::from_str("flat"), Some(EqPreset::Flat));
        assert_eq!(EqPreset::from_str("rock"), Some(EqPreset::Rock));
        assert_eq!(EqPreset::from_str("bassboost"), Some(EqPreset::BassBoost));
        assert_eq!(EqPreset::from_str("invalid"), None);
    }

    #[test]
    fn test_eq_preset_mpv_string() {
        let s = format_supereq_string(EqPreset::Flat.bands_18());
        assert!(s.contains("superequalizer="));
        // Flat is all 0 dB -> 1.0 multiplier.
        assert!(s.contains("1b=1.000"));
        assert!(s.contains("18b=1.000"));
    }

    #[test]
    fn test_map_10_to_18_bands() {
        let bands_10 = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let bands_18 = map_10_to_18_bands(&bands_10);
        // Anchors are pinned exactly...
        assert_eq!(bands_18[0], 1);
        assert_eq!(bands_18[16], 10);
        // ...and the in-between bands are interpolated, not left at 0.
        assert!(bands_18[4] >= bands_18[3] && bands_18[4] <= bands_18[5]);
        assert_ne!(bands_18[4], 0);
    }

    #[test]
    fn test_map_no_interior_zero_pinning() {
        // A constant curve in must stay constant across ALL 18 bands, including
        // the interpolated interior bands and the extrapolated top band.
        let bands_18 = map_10_to_18_bands(&[6; 10]);
        assert_eq!(bands_18, [6; 18]);
    }

    #[test]
    fn test_map_interpolates_between_anchors() {
        // Two nonzero neighbours must not leave the band between them at 0.
        let bands_10 = [0, 0, 0, 10, 10, 0, 0, 0, 0, 0];
        let bands_18 = map_10_to_18_bands(&bands_10);
        // Band 4 (262Hz) sits between anchors 3 and 5, both +10.
        assert_eq!(bands_18[4], 10);
    }

    #[test]
    fn test_supereq_freqs_invariants() {
        assert_eq!(SUPEREQ_FREQS.len(), 18);
        // Frequencies are whole numbers; cast to compare (avoids float ==).
        assert_eq!(SUPEREQ_FREQS[0] as i32, 65);
        assert_eq!(SUPEREQ_FREQS[16] as i32, 16744);
        assert_eq!(SUPEREQ_FREQS[17] as i32, 20000);
        // Strictly increasing so log-frequency interpolation is well-defined.
        for pair in SUPEREQ_FREQS.windows(2) {
            assert!(pair[1] > pair[0]);
        }
    }

    #[test]
    fn test_clamp_extended() {
        // ±20dB now applies verbatim: +20 -> 10^(20/20)=10.000,
        // -20 -> 10^(-20/20)=0.100, both inside superequalizer's [0, 20].
        let mut bands = [0i32; 18];
        bands[0] = 20;
        bands[1] = -20;
        let s = format_supereq_string(bands);
        assert!(s.contains("1b=10.000"));
        assert!(s.contains("2b=0.100"));
    }

    #[test]
    fn test_filter_string_pads_short_slice() {
        // A 10-band slice must still yield a full 18-band filter string.
        let s = filter_string(&[0i32; 10]);
        assert!(s.contains("1b=1.000"));
        assert!(s.contains("18b=1.000"));
    }
}
