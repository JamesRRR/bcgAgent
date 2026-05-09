//! Push-to-talk streaming transcription session.
//!
//! Frontend pipeline (Walkthrough page):
//! 1. User holds space, MediaRecorder produces 16kHz mono WAV chunks every
//!    ~750ms.
//! 2. Each chunk is sent to `transcribe_chunk(session_id, wav_bytes)`. The
//!    backend appends raw 16-bit samples to an in-memory buffer keyed by
//!    session_id. After every chunk we re-run whisper-cli on the running
//!    buffer (a few hundred ms each pass for short utterances) and emit the
//!    text via the `transcribe:partial` Tauri event.
//! 3. On release, the frontend calls `transcribe_finalize(session_id)`.
//!    We do one last whisper-cli pass with `--no-fallback` for accuracy and
//!    return the final transcript. The session buffer is then dropped.
//!
//! This isn't true-streaming (each pass re-decodes the cumulative WAV) but
//! short utterances (~5-10s) decode in <500ms on Apple Silicon, which is the
//! perceived latency the user cares about.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::error::{AppError, AppResult};

/// Sample buffer for one push-to-talk session. Samples are i16 PCM @ 16kHz.
#[derive(Default)]
pub struct StreamSession {
    pub samples: Vec<i16>,
    pub lang_hint: String,
}

/// Process-wide registry of active streaming sessions.
#[derive(Clone, Default)]
pub struct StreamRegistry {
    inner: Arc<Mutex<HashMap<String, StreamSession>>>,
}

impl StreamRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&self, session_id: &str, lang_hint: &str) {
        let mut g = self.inner.lock();
        g.insert(
            session_id.to_string(),
            StreamSession {
                samples: Vec::with_capacity(16_000 * 4),
                lang_hint: lang_hint.to_string(),
            },
        );
    }

    pub fn append(&self, session_id: &str, samples: &[i16]) -> AppResult<usize> {
        let mut g = self.inner.lock();
        let s = g
            .get_mut(session_id)
            .ok_or_else(|| AppError::Audio(format!("no streaming session {session_id}")))?;
        s.samples.extend_from_slice(samples);
        Ok(s.samples.len())
    }

    pub fn snapshot(&self, session_id: &str) -> Option<(Vec<i16>, String)> {
        let g = self.inner.lock();
        let s = g.get(session_id)?;
        Some((s.samples.clone(), s.lang_hint.clone()))
    }

    pub fn finish(&self, session_id: &str) -> Option<(Vec<i16>, String)> {
        let mut g = self.inner.lock();
        g.remove(session_id).map(|s| (s.samples, s.lang_hint))
    }
}

/// Decode a WAV blob (any sample format we are likely to receive from a
/// browser MediaRecorder → wav.ts) to i16 mono samples at 16kHz. The
/// frontend already runs `blobToWav16k` so input is canonical 16kHz mono
/// 16-bit PCM, but we accept and resample if the spec ever drifts.
pub fn decode_wav_to_i16(bytes: &[u8]) -> AppResult<Vec<i16>> {
    use std::io::Cursor;
    let cursor = Cursor::new(bytes);
    let mut reader =
        hound::WavReader::new(cursor).map_err(|e| AppError::Audio(format!("wav decode: {e}")))?;
    let spec = reader.spec();
    if spec.sample_rate != 16_000 || spec.channels != 1 {
        return Err(AppError::Audio(format!(
            "expected 16kHz mono, got {}Hz/{} channels",
            spec.sample_rate, spec.channels
        )));
    }
    let samples: Vec<i16> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Audio(format!("wav samples: {e}")))?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<f32>, _>>()
            .map_err(|e| AppError::Audio(format!("wav samples: {e}")))?
            .into_iter()
            .map(|f| (f.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect(),
    };
    Ok(samples)
}

/// Write i16 mono samples to a WAV file at 16kHz under our app-owned audio
/// dir so whisper-cli can open it. We deliberately avoid `tempfile` because
/// the OS tempdir can be a sandbox-managed subdir (e.g. context-mode's
/// `.ctx-mode-XXX/`) that gets cleaned up under our feet, breaking the path
/// before whisper-cli runs. Caller is responsible for deleting the file.
pub fn write_temp_wav(samples: &[i16]) -> AppResult<PathBuf> {
    let dir = crate::paths::audio_dir().join("stream");
    std::fs::create_dir_all(&dir).map_err(AppError::Io)?;
    let path = dir.join(format!("{}.wav", uuid::Uuid::new_v4()));
    {
        let f = std::fs::File::create(&path).map_err(AppError::Io)?;
        let writer = std::io::BufWriter::new(f);
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::new(writer, spec)
            .map_err(|e| AppError::Audio(format!("wav writer: {e}")))?;
        for s in samples {
            w.write_sample(*s)
                .map_err(|e| AppError::Audio(format!("wav sample: {e}")))?;
        }
        w.finalize()
            .map_err(|e| AppError::Audio(format!("wav finalize: {e}")))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_wav(samples: &[i16]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut w = hound::WavWriter::new(&mut buf, spec).unwrap();
            for s in samples {
                w.write_sample(*s).unwrap();
            }
            w.finalize().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn decodes_canonical_int_wav() {
        let bytes = synthetic_wav(&[0i16, 1000, -1000, 2000]);
        let s = decode_wav_to_i16(&bytes).unwrap();
        assert_eq!(s, vec![0, 1000, -1000, 2000]);
    }

    #[test]
    fn rejects_wrong_sample_rate() {
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::new(&mut buf, spec).unwrap();
        w.write_sample(0i16).unwrap();
        w.finalize().unwrap();
        let bytes = buf.into_inner();
        assert!(decode_wav_to_i16(&bytes).is_err());
    }

    #[test]
    fn registry_appends_and_finishes() {
        let r = StreamRegistry::new();
        r.start("sess1", "auto");
        r.append("sess1", &[1, 2, 3]).unwrap();
        r.append("sess1", &[4, 5]).unwrap();
        let (samples, lang) = r.snapshot("sess1").unwrap();
        assert_eq!(samples, vec![1, 2, 3, 4, 5]);
        assert_eq!(lang, "auto");
        let (samples, _) = r.finish("sess1").unwrap();
        assert_eq!(samples.len(), 5);
        assert!(r.snapshot("sess1").is_none());
    }
}
