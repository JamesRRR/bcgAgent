//! Voice round-trip integration test.
//!
//! Generates Chinese audio via macOS `say`, converts to 16 kHz mono WAV via
//! `afconvert`, runs `audio::transcribe` (downloads whisper model ~570MB on
//! first run), and asserts the transcript contains the source tokens.
//!
//! Requires:
//!   - macOS (uses `say` and `afconvert`)
//!   - whisper-cli on PATH (`brew install whisper-cpp`)
//!
//! Run with:
//!   cargo test --test voice_roundtrip -- --ignored --nocapture

use std::path::PathBuf;
use std::process::Command;

use bcgagent_lib::audio;

fn synthesize_chinese_wav(text: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    let aiff = dir.join("bcg-roundtrip.aiff");
    let wav = dir.join("bcg-roundtrip.wav");
    // 1) say -> aiff
    let s = Command::new("say")
        .args(["-v", "Tingting", "-o"])
        .arg(&aiff)
        .arg(text)
        .status()
        .expect("spawn say");
    assert!(s.success(), "say failed");
    // 2) aiff -> 16k mono LE PCM WAV
    let s = Command::new("afconvert")
        .arg(&aiff)
        .arg(&wav)
        .args(["-d", "LEI16@16000", "-f", "WAVE"])
        .status()
        .expect("spawn afconvert");
    assert!(s.success(), "afconvert failed");
    let _ = std::fs::remove_file(&aiff);
    wav
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn say_to_whisper_roundtrip_zh() {
    let source = "你好，我们来玩卡坦岛。";
    eprintln!("source: {}", source);

    let wav = synthesize_chinese_wav(source);
    eprintln!("wav: {}", wav.display());

    let transcript = audio::transcribe(&wav, "zh")
        .await
        .expect("whisper transcribe failed");

    eprintln!("transcript: {:?}", transcript);

    let lower = transcript.to_lowercase();
    let hits: Vec<&str> = ["你好", "卡坦", "卡坦岛", "玩"]
        .iter()
        .filter(|kw| lower.contains(&kw.to_lowercase()))
        .copied()
        .collect();
    assert!(
        !hits.is_empty(),
        "transcript missing all source tokens: {:?}",
        transcript
    );
    eprintln!("matched tokens: {:?}", hits);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn tts_speak_completes() {
    // Smoke: macOS `say` runs to completion for a short Chinese phrase.
    let h = audio::speak("测试", "zh").expect("speak");
    h.wait().expect("wait");
}
