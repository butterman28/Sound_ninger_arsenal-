// src/recording.rs
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use parking_lot::RwLock;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::audio::AudioAsset;
use crate::gui::NUM_STEPS;
use crate::adsr::ADSREnvelope;

#[derive(Clone, Debug, PartialEq)]
pub enum RecordState {
    Idle,
    Recording,
    Recorded,
}

#[derive(Clone, Debug)]
pub struct InputDevice {
    pub host_id:     cpal::HostId,
    pub host_name:   String,
    pub device_name: String,
    pub label: String,
}

pub struct RecordingTrack {
    pub device_label: Option<String>,
    pub state:        RecordState,
    pub asset:        Option<Arc<AudioAsset>>,
    pub steps:        [bool; NUM_STEPS],
    pub adsr:         ADSREnvelope,
    pub adsr_enabled: bool,
    pub muted:        bool,
    pub take_number:  u32,
}

impl RecordingTrack {
    pub fn new() -> Self {
        Self {
            device_label: None,
            state:        RecordState::Idle,
            asset:        None,
            steps:        [false; NUM_STEPS],
            adsr:         ADSREnvelope::default(),
            adsr_enabled: false,
            muted:        false,
            take_number:  1,
        }
    }

    pub fn short_name(&self) -> String {
        if let Some(a) = &self.asset {
            let n = &a.file_name;
            if n.len() > 15 { format!("{}…", &n[..13]) } else { n.clone() }
        } else if let Some(lbl) = &self.device_label {
            let dev = lbl.split(": ").nth(1).unwrap_or(lbl);
            if dev.len() > 15 { format!("{}…", &dev[..13]) } else { dev.to_string() }
        } else {
            "No input".to_string()
        }
    }

    pub fn duration_secs(&self) -> Option<f32> {
        self.asset.as_ref().map(|a| a.frames as f32 / a.sample_rate as f32)
    }
}

pub struct RecordingManager {
    pub buffer:       Arc<Mutex<Vec<f32>>>,
    pub is_recording: Arc<AtomicBool>,
    pub stream:       Arc<RwLock<Option<cpal::Stream>>>,
    pub sample_rate:  Arc<RwLock<u32>>,
    pub channels:     Arc<RwLock<u16>>,
    pub peak:         Arc<RwLock<f32>>,
}

impl RecordingManager {
    pub fn new() -> Self {
        Self {
            buffer:       Arc::new(Mutex::new(Vec::new())),
            is_recording: Arc::new(AtomicBool::new(false)),
            stream:       Arc::new(RwLock::new(None)),
            sample_rate:  Arc::new(RwLock::new(44100)),
            channels:     Arc::new(RwLock::new(1)),
            peak:         Arc::new(RwLock::new(0.0)),
        }
    }

    pub fn list_input_devices() -> Vec<InputDevice> {
        let mut out = Vec::new();
        for host_id in cpal::available_hosts() {
            let host_name = format!("{:?}", host_id);
            let host = match cpal::host_from_id(host_id) {
                Ok(h) => h,
                Err(_) => continue,
            };
            let devices = match host.input_devices() {
                Ok(d) => d,
                Err(_) => continue,
            };
            for device in devices {
                if let Ok(device_name) = device.name() {
                    let label = format!("{}: {}", host_name, device_name);
                    out.push(InputDevice { host_id, host_name: host_name.clone(), device_name, label });
                }
            }
        }
        out
    }

    pub fn start(&self, dev: &InputDevice) -> Result<(), String> {
        self.stop();

        let host = cpal::host_from_id(dev.host_id)
            .map_err(|e| format!("Host error: {:?}", e))?;

        let device = host.input_devices()
            .map_err(|e| format!("Device list: {}", e))?
            .find(|d| d.name().map(|n| n == dev.device_name).unwrap_or(false))
            .ok_or_else(|| format!("Device '{}' not found (try Refresh)", dev.device_name))?;

        let cfg = device.default_input_config()
            .map_err(|e| format!("Input config: {}", e))?;

        *self.sample_rate.write() = cfg.sample_rate().0;
        *self.channels.write()    = cfg.channels();

        self.buffer.lock().unwrap().clear();
        self.is_recording.store(true, Ordering::Relaxed);

        let scfg: cpal::StreamConfig = cfg.clone().into();

        let stream = {
            let buf  = self.buffer.clone();
            let rec  = self.is_recording.clone();
            let peak = self.peak.clone();

            match cfg.sample_format() {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    &scfg,
                    move |data: &[f32], _| {
                        if !rec.load(Ordering::Relaxed) { return; }
                        let p = data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                        *peak.write() = p;
                        buf.lock().unwrap().extend_from_slice(data);
                    },
                    |e| eprintln!("Rec stream error: {}", e), None,
                ),
                cpal::SampleFormat::I16 => device.build_input_stream(
                    &scfg,
                    {
                        let buf2 = buf.clone(); let rec2 = rec.clone(); let peak2 = peak.clone();
                        move |data: &[i16], _| {
                            if !rec2.load(Ordering::Relaxed) { return; }
                            let s: Vec<f32> = data.iter().map(|&x| x as f32 / 32767.0).collect();
                            let p = s.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
                            *peak2.write() = p;
                            buf2.lock().unwrap().extend_from_slice(&s);
                        }
                    },
                    |e| eprintln!("Rec stream error: {}", e), None,
                ),
                cpal::SampleFormat::U16 => device.build_input_stream(
                    &scfg,
                    {
                        let buf3 = buf.clone(); let rec3 = rec.clone(); let peak3 = peak.clone();
                        move |data: &[u16], _| {
                            if !rec3.load(Ordering::Relaxed) { return; }
                            let s: Vec<f32> = data.iter().map(|&x| x as f32 / 32767.5 - 1.0).collect();
                            let p = s.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
                            *peak3.write() = p;
                            buf3.lock().unwrap().extend_from_slice(&s);
                        }
                    },
                    |e| eprintln!("Rec stream error: {}", e), None,
                ),
                fmt => return Err(format!("Unsupported input sample format: {:?}", fmt)),
            }
        }.map_err(|e| format!("Build input stream: {}", e))?;

        stream.play().map_err(|e| format!("Start stream: {}", e))?;
        *self.stream.write() = Some(stream);
        Ok(())
    }

    pub fn stop(&self) {
        self.is_recording.store(false, Ordering::Relaxed);
        *self.stream.write() = None;
        *self.peak.write() = 0.0;
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::Relaxed)
    }

    pub fn peak(&self) -> f32 {
        *self.peak.read()
    }

    pub fn take_asset(&self, file_name: String) -> Option<Arc<AudioAsset>> {
        let pcm = {
            let mut buf = self.buffer.lock().ok()?;
            std::mem::take(&mut *buf)
        };
        if pcm.is_empty() { return None; }
        let sr = *self.sample_rate.read();
        let ch = *self.channels.read();
        Some(Arc::new(AudioAsset {
            frames: pcm.len() as u64 / ch.max(1) as u64,
            pcm,
            sample_rate: sr,
            channels: ch,
            file_name,
            sample_uuid: uuid::Uuid::new_v4(), // ✅ fresh UUID for every recording
        }))
    }

    pub fn recorded_secs(&self) -> f32 {
        let sr = *self.sample_rate.read() as f32;
        let ch = (*self.channels.read()).max(1) as f32;
        if let Ok(buf) = self.buffer.lock() {
            buf.len() as f32 / sr / ch
        } else {
            0.0
        }
    }
}