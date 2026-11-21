//! Waveform peak precomputation and on-disk cache.
//!
//! mpv plays audio in a separate subprocess and exposes no raw PCM over its
//! JSON IPC, so a real, seekable waveform cannot be drawn from live samples.
//! Instead we decode a track's audio once, downsample it to a small array of
//! normalised peak buckets, and cache that array to disk keyed by the track's
//! id — mirroring the artwork cache. The frontend then draws (and seeks over)
//! the peaks on a canvas.
//!
//! # Decode source
//!
//! - A locally downloaded file is decoded directly (offline-friendly).
//! - Otherwise the caller fetches a small server-transcoded **MP3** and decodes
//!   that: it keeps the download tiny (~a few MB) and sidesteps Opus, which
//!   symphonia cannot decode.
//!
//! Peaks are cheap (a few hundred floats) and a track's waveform never changes,
//! so the cache is immutable once written.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;

/// Number of peak buckets stored per track.
///
/// Enough to render a crisp waveform at the full-screen width; the frontend
/// groups buckets down for narrower surfaces.
pub const NUM_BUCKETS: usize = 400;

/// Bumped when the peak format changes so stale caches are recomputed.
/// v2: RMS loudness envelope (v1 used peak/max, which looked flat on loud masters).
const WAVEFORM_VERSION: u32 = 2;

/// Frames folded into one intermediate "mini-peak" before rebucketing.
///
/// Bounds decode-time memory to a few thousand floats regardless of track
/// length (a 4-minute track at 44.1kHz is ~10k mini-peaks).
const FOLD_WINDOW: usize = 1024;

/// On-disk cache envelope. Versioned so a format change invalidates old files.
#[derive(Serialize, Deserialize)]
struct CachedWaveform {
    version: u32,
    peaks: Vec<f32>,
}

/// Resolve the cache file path for a track's waveform.
fn cache_path(cache_dir: &Path, id: &str) -> PathBuf {
    let safe_id: String = id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect();
    cache_dir.join(format!("{}.json", safe_id))
}

/// Read cached peaks for a track, if present and of the current version.
pub async fn read_cache(cache_dir: &Path, id: &str) -> Option<Vec<f32>> {
    let path = cache_path(cache_dir, id);
    let bytes = tokio::fs::read(&path).await.ok()?;
    let cached: CachedWaveform = serde_json::from_slice(&bytes).ok()?;
    if cached.version != WAVEFORM_VERSION || cached.peaks.is_empty() {
        return None;
    }
    Some(cached.peaks)
}

/// Monotonic counter giving each write a unique temp filename, so two
/// concurrent writers (e.g. main + mini windows) never share a temp path.
static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Persist computed peaks for a track. Best-effort — failures are logged only.
///
/// Writes to a unique temp file then atomically renames into place, so a
/// concurrent reader (or a crash mid-write) can never observe a truncated
/// JSON file.
pub async fn write_cache(cache_dir: &Path, id: &str, peaks: &[f32]) {
    if let Err(e) = tokio::fs::create_dir_all(cache_dir).await {
        tracing::warn!("Failed to create waveform cache dir: {}", e);
        return;
    }
    let envelope = CachedWaveform {
        version: WAVEFORM_VERSION,
        peaks: peaks.to_vec(),
    };
    let bytes = match serde_json::to_vec(&envelope) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Failed to serialise waveform for {}: {}", id, e);
            return;
        }
    };

    let final_path = cache_path(cache_dir, id);
    let seq = WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = final_path.with_extension(format!("{seq}.tmp"));

    if let Err(e) = tokio::fs::write(&tmp_path, &bytes).await {
        tracing::warn!("Failed to write waveform temp for {}: {}", id, e);
        return;
    }
    if let Err(e) = tokio::fs::rename(&tmp_path, &final_path).await {
        tracing::warn!("Failed to commit waveform for {}: {}", id, e);
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }
}

/// Decode an audio source and downsample to `num_buckets` normalised peaks
/// (0.0–1.0).
///
/// CPU-bound (and blocking I/O when the source is a `File`) — call from
/// `spawn_blocking`. Streaming a `File` avoids buffering a large lossless file
/// into RAM. `ext_hint` (e.g. `"mp3"`, `"flac"`) helps symphonia pick a demuxer
/// for a source whose container it can't sniff. Returns an error string on an
/// undecodable/empty stream (e.g. Opus).
pub fn compute_peaks(
    source: Box<dyn MediaSource>,
    ext_hint: Option<&str>,
    num_buckets: usize,
) -> Result<Vec<f32>, String> {
    let mss = MediaSourceStream::new(source, Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = ext_hint {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("probe failed: {e}"))?;

    // `default_track` falls back to the first audio track with a known codec, so
    // this covers what the old `codec != CODEC_TYPE_NULL` scan did.
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "no decodable audio track".to_string())?;
    let track_id = track.id;
    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| "audio track has no codec parameters".to_string())?;

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(audio_params, &AudioDecoderOptions::default())
        .map_err(|e| format!("decoder init failed: {e}"))?;

    // Fold decoded frames into RMS mini-peaks (perceived loudness) to bound
    // memory. RMS — not peak/max — is what gives a waveform its dynamic shape:
    // on a loud, brick-walled master almost every window hits full-scale peak,
    // so a peak envelope looks like a flat block, whereas RMS tracks the actual
    // quiet/loud variation across the track.
    let mut mini: Vec<f32> = Vec::new();
    let mut sum_sq = 0.0f64; // f64 so summing ~1024 squares doesn't lose precision
    let mut sample_count = 0usize;
    let mut frame_count = 0usize;
    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            // Clean end of stream, or a mid-stream reset we can't continue past.
            Ok(None) => break,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(format!("packet read failed: {e}")),
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            // A single corrupt frame shouldn't abort the whole waveform.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(format!("decode failed: {e}")),
        };

        let channels = decoded.spec().channels().count().max(1);

        // Resize the interleaving buffer to exactly this packet's sample count,
        // then copy the frames out in interleaved order.
        samples.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut samples);

        for frame in samples.chunks(channels) {
            for &s in frame {
                sum_sq += (s as f64) * (s as f64);
            }
            sample_count += frame.len();
            frame_count += 1;
            if frame_count >= FOLD_WINDOW {
                let rms = if sample_count > 0 {
                    (sum_sq / sample_count as f64).sqrt() as f32
                } else {
                    0.0
                };
                mini.push(rms);
                sum_sq = 0.0;
                sample_count = 0;
                frame_count = 0;
            }
        }
    }
    if frame_count > 0 && sample_count > 0 {
        mini.push((sum_sq / sample_count as f64).sqrt() as f32);
    }

    if mini.is_empty() {
        return Err("decoded stream produced no samples".to_string());
    }

    Ok(rebucket_and_normalise(&mini, num_buckets))
}

/// Collapse the mini RMS values into exactly `num_buckets` by averaging each
/// bucket's slice, then normalise to the loudest bucket (linear — no curve, so
/// the quiet/loud dynamics are preserved rather than flattened).
fn rebucket_and_normalise(mini: &[f32], num_buckets: usize) -> Vec<f32> {
    if num_buckets == 0 {
        return Vec::new();
    }
    let n = mini.len();
    if n == 0 {
        return vec![0.0; num_buckets];
    }
    let mut buckets = vec![0.0f32; num_buckets];
    for (i, bucket) in buckets.iter_mut().enumerate() {
        let start = i * n / num_buckets;
        let end = (((i + 1) * n / num_buckets).max(start + 1)).min(n);
        let slice = &mini[start..end];
        *bucket = slice.iter().sum::<f32>() / slice.len() as f32;
    }

    let max = buckets.iter().copied().fold(0.0f32, f32::max);
    if max > 0.0 {
        for b in buckets.iter_mut() {
            *b /= max;
        }
    }
    buckets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebucket_downsamples_to_exact_count() {
        let mini: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        let out = rebucket_and_normalise(&mini, NUM_BUCKETS);
        assert_eq!(out.len(), NUM_BUCKETS);
        // Loudest bucket normalises to 1.0.
        assert!((out.last().copied().unwrap() - 1.0).abs() < 1e-6);
        // All buckets are within the normalised range.
        assert!(out.iter().all(|&v| (0.0..=1.0).contains(&v)));
    }

    #[test]
    fn rebucket_upsamples_short_input_without_panicking() {
        // Fewer mini-peaks than buckets must still fill every bucket.
        let mini = vec![0.5f32, 1.0, 0.25];
        let out = rebucket_and_normalise(&mini, NUM_BUCKETS);
        assert_eq!(out.len(), NUM_BUCKETS);
        assert!(out.iter().all(|&v| v.is_finite()));
    }

    #[test]
    fn rebucket_all_silence_stays_zero() {
        let mini = vec![0.0f32; 500];
        let out = rebucket_and_normalise(&mini, 100);
        assert_eq!(out.len(), 100);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn compute_peaks_rejects_garbage() {
        // Not audio — probe/decoder must fail cleanly, never panic.
        let bytes = vec![0u8; 4096];
        let source = Box::new(std::io::Cursor::new(bytes));
        assert!(compute_peaks(source, Some("mp3"), NUM_BUCKETS).is_err());
    }

    /// Build a 16-bit stereo PCM WAV in memory from interleaved frames.
    fn wav_stereo_16bit(rate: u32, frames: &[(i16, i16)]) -> Vec<u8> {
        let data_len = (frames.len() * 2 * 2) as u32;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&2u16.to_le_bytes()); // channels
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 4).to_le_bytes()); // byte rate
        wav.extend_from_slice(&4u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for (l, r) in frames {
            wav.extend_from_slice(&l.to_le_bytes());
            wav.extend_from_slice(&r.to_le_bytes());
        }
        wav
    }

    #[test]
    fn compute_peaks_tracks_loudness_of_real_audio() {
        // Drives the whole decode path (probe → decoder → interleaved copy → RMS)
        // rather than asserting it merely compiles: a loud first half and a quiet
        // second half must come back as a visibly louder first half.
        const RATE: u32 = 8000;
        let total = (RATE as usize) * 2; // two seconds
        let frames: Vec<(i16, i16)> = (0..total)
            .map(|i| {
                let phase = (i as f32) * std::f32::consts::TAU * 440.0 / RATE as f32;
                let amplitude = if i < total / 2 { 20_000.0 } else { 2_000.0 };
                let s = (phase.sin() * amplitude) as i16;
                (s, s)
            })
            .collect();

        let wav = wav_stereo_16bit(RATE, &frames);
        let peaks = compute_peaks(
            Box::new(std::io::Cursor::new(wav)),
            Some("wav"),
            NUM_BUCKETS,
        )
        .expect("generated WAV must decode");

        assert_eq!(peaks.len(), NUM_BUCKETS);
        assert!(peaks.iter().all(|v| (0.0..=1.0).contains(v)));

        // Sample well inside each half so the boundary bucket can't skew it.
        let loud = peaks[NUM_BUCKETS / 4];
        let quiet = peaks[NUM_BUCKETS * 3 / 4];
        assert!(
            loud > 0.5,
            "loud half should be near full scale, got {loud}"
        );
        assert!(
            quiet < loud / 2.0,
            "quiet half ({quiet}) should be well below the loud half ({loud})"
        );
    }
}
