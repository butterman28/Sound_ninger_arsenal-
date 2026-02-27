use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use parking_lot::RwLock;
use symphonia::core::{
    audio::{AudioBufferRef, Signal},
    codecs::{DecoderOptions, CODEC_TYPE_NULL},
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

#[derive(Debug, Clone)]
pub struct AudioAsset {
    pub pcm: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub file_name: String,
    pub sample_uuid: uuid::Uuid,  // ✅ Every loaded asset carries its own UUID
}

#[derive(Debug, Clone)]
pub struct WaveformAnalysis {
    pub min_max_buckets: Vec<(f32, f32)>,
    pub sample_rate: u32,
}

pub struct AudioManager {
    assets: RwLock<std::collections::HashMap<String, Arc<AudioAsset>>>,
}

impl AudioManager {
    pub fn new() -> Self {
        Self {
            assets: RwLock::new(std::collections::HashMap::new()),
        }
    }

    pub fn load_audio(&self, path: &str) -> Result<Arc<AudioAsset>, Box<dyn std::error::Error>> {
        // NOTE: We intentionally do NOT return cached assets here.
        // Returning a cached asset would mean two tracks loaded from the
        // same file share a UUID → they'd share chop markers. Instead we
        // always decode fresh and assign a brand-new UUID so every load is
        // treated as a clean slate ("tabula rasa").
        //
        // If you later want to re-enable caching for large files, generate a
        // new UUID after cache hit and return a cloned asset with that new UUID.

        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe().format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?;

        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("no valid audio track found")?;
        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.ok_or("unknown sample rate")?;
        let channels = track
            .codec_params
            .channels
            .ok_or("unknown channels")?
            .count() as u16;

        let mut decoder =
            symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

        let mut pcm: Vec<f32> = Vec::new();
        let mut frames: u64 = 0;

        loop {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(_) => break,
            };
            if packet.track_id() != track_id { continue; }
            match decoder.decode(&packet) {
                Ok(decoded) => {
                    match decoded {
                        AudioBufferRef::F32(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels { pcm.push(buf.chan(ch)[frame]); }
                            }
                            frames += buf.frames() as u64;
                        }
                        AudioBufferRef::U8(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels {
                                    pcm.push(buf.chan(ch)[frame] as f32 / 127.5 - 1.0);
                                }
                            }
                            frames += buf.frames() as u64;
                        }
                        AudioBufferRef::S8(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels {
                                    pcm.push(buf.chan(ch)[frame] as f32 / 127.0);
                                }
                            }
                            frames += buf.frames() as u64;
                        }
                        AudioBufferRef::U16(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels {
                                    pcm.push(buf.chan(ch)[frame] as f32 / 32767.5 - 1.0);
                                }
                            }
                            frames += buf.frames() as u64;
                        }
                        AudioBufferRef::S16(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels {
                                    pcm.push(buf.chan(ch)[frame] as f32 / 32767.0);
                                }
                            }
                            frames += buf.frames() as u64;
                        }
                        AudioBufferRef::U24(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels {
                                    let val = buf.chan(ch)[frame];
                                    pcm.push((val.inner() as f32) / 8388607.5 - 1.0);
                                }
                            }
                            frames += buf.frames() as u64;
                        }
                        AudioBufferRef::S24(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels {
                                    let val = buf.chan(ch)[frame];
                                    pcm.push((val.inner() as f32) / 8388607.0);
                                }
                            }
                            frames += buf.frames() as u64;
                        }
                        AudioBufferRef::U32(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels {
                                    pcm.push(buf.chan(ch)[frame] as f32 / 2147483647.5 - 1.0);
                                }
                            }
                            frames += buf.frames() as u64;
                        }
                        AudioBufferRef::S32(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels {
                                    pcm.push(buf.chan(ch)[frame] as f32 / 2147483647.0);
                                }
                            }
                            frames += buf.frames() as u64;
                        }
                        AudioBufferRef::F64(buf) => {
                            let channels = buf.spec().channels.count();
                            for frame in 0..buf.frames() {
                                for ch in 0..channels {
                                    pcm.push(buf.chan(ch)[frame] as f32);
                                }
                            }
                            frames += buf.frames() as u64;
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        if pcm.is_empty() {
            return Err("no audio samples decoded".into());
        }

        // ✅ Fresh UUID every time — even for the same file path.
        // This is the guarantee that reloading a file is a clean slate.
        let asset = Arc::new(AudioAsset {
            pcm,
            sample_rate,
            channels,
            frames,
            file_name: Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("unknown")
                .to_string(),
            sample_uuid: uuid::Uuid::new_v4(),
        });

        println!("Loaded: {} (uuid={})", path, asset.sample_uuid);
        Ok(asset)
    }

    pub fn analyze_waveform(&self, asset: &AudioAsset, buckets: usize) -> WaveformAnalysis {
        if asset.pcm.is_empty() || buckets == 0 {
            return WaveformAnalysis {
                min_max_buckets: vec![(0.0, 0.0); buckets],
                sample_rate: asset.sample_rate,
            };
        }

        let samples = &asset.pcm;
        let bucket_size = (samples.len() as f32 / buckets as f32).max(1.0) as usize;

        let min_max_buckets = (0..buckets)
            .map(|i| {
                let start = i * bucket_size;
                let end = (start + bucket_size).min(samples.len());
                let slice = &samples[start..end];
                let (min, max) = slice.iter().fold((0.0f32, 0.0f32), |(min, max), &s| {
                    (min.min(s), max.max(s))
                });
                (min, max)
            })
            .collect();

        WaveformAnalysis {
            min_max_buckets,
            sample_rate: asset.sample_rate,
        }
    }
}