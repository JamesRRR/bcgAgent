// Encode an audio Blob (e.g. WebM/Opus from MediaRecorder) into a 16 kHz mono
// PCM WAV (RIFF). whisper.cpp expects 16 kHz mono PCM.
export async function blobToWav16k(blob: Blob): Promise<Uint8Array> {
  const arr = await blob.arrayBuffer();
  // Use a temporary AudioContext just to decode; sampleRate hint asks the
  // browser to resample on the way in.
  const tmp = new AudioContext();
  const decoded = await tmp.decodeAudioData(arr.slice(0));
  await tmp.close();

  const targetRate = 16000;
  const offline = new OfflineAudioContext(
    1,
    Math.ceil(decoded.duration * targetRate),
    targetRate,
  );
  const src = offline.createBufferSource();
  // Downmix to mono manually if needed
  const monoBuffer = offline.createBuffer(
    1,
    decoded.length,
    decoded.sampleRate,
  );
  const tmpData = monoBuffer.getChannelData(0);
  if (decoded.numberOfChannels === 1) {
    tmpData.set(decoded.getChannelData(0));
  } else {
    const l = decoded.getChannelData(0);
    const r = decoded.getChannelData(1);
    for (let i = 0; i < decoded.length; i++) {
      tmpData[i] = (l[i] + r[i]) * 0.5;
    }
  }
  src.buffer = monoBuffer;
  src.connect(offline.destination);
  src.start(0);
  const rendered = await offline.startRendering();
  return encodeWavPCM16(rendered.getChannelData(0), targetRate);
}

function encodeWavPCM16(samples: Float32Array, sampleRate: number): Uint8Array {
  const numChannels = 1;
  const bitsPerSample = 16;
  const byteRate = (sampleRate * numChannels * bitsPerSample) / 8;
  const blockAlign = (numChannels * bitsPerSample) / 8;
  const dataSize = samples.length * 2;
  const buffer = new ArrayBuffer(44 + dataSize);
  const view = new DataView(buffer);

  // RIFF header
  writeString(view, 0, "RIFF");
  view.setUint32(4, 36 + dataSize, true);
  writeString(view, 8, "WAVE");
  // fmt chunk
  writeString(view, 12, "fmt ");
  view.setUint32(16, 16, true); // PCM chunk size
  view.setUint16(20, 1, true); // format = PCM
  view.setUint16(22, numChannels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, byteRate, true);
  view.setUint16(32, blockAlign, true);
  view.setUint16(34, bitsPerSample, true);
  // data chunk
  writeString(view, 36, "data");
  view.setUint32(40, dataSize, true);

  // PCM 16-bit samples
  let offset = 44;
  for (let i = 0; i < samples.length; i++, offset += 2) {
    let s = Math.max(-1, Math.min(1, samples[i]));
    s = s < 0 ? s * 0x8000 : s * 0x7fff;
    view.setInt16(offset, s | 0, true);
  }
  return new Uint8Array(buffer);
}

function writeString(view: DataView, offset: number, str: string) {
  for (let i = 0; i < str.length; i++) {
    view.setUint8(offset + i, str.charCodeAt(i));
  }
}
