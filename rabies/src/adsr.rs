use std::sync::Arc;

/// ADSR Envelope phases
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ADSRPhase {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

/// ADSR Envelope parameters
#[derive(Clone, Copy, Debug)]
pub struct ADSREnvelope {
    pub attack: f32,    // 0.0 - 2.0 seconds
    pub decay: f32,     // 0.0 - 2.0 seconds
    pub sustain: f32,   // 0.0 - 1.0 (level)
    pub release: f32,   // 0.0 - 3.0 seconds
}

impl Default for ADSREnvelope {
    fn default() -> Self {
        Self {
            attack: 0.01,
            decay: 0.1,
            sustain: 0.8,
            release: 0.2,
        }
    }
}

impl ADSREnvelope {
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self { attack, decay, sustain, release }
    }

    pub fn percussive() -> Self {
        Self {
            attack: 0.001,
            decay: 0.1,
            sustain: 0.3,
            release: 0.05,
        }
    }

    pub fn pad() -> Self {
        Self {
            attack: 0.3,
            decay: 0.2,
            sustain: 0.7,
            release: 0.5,
        }
    }

    pub fn pluck() -> Self {
        Self {
            attack: 0.001,
            decay: 0.3,
            sustain: 0.1,
            release: 0.1,
        }
    }
}

/// Voice envelope state tracker
#[derive(Clone, Debug)]
pub struct EnvelopeState {
    pub phase: ADSRPhase,
    pub elapsed: f64,
    pub gate_open: bool,
}

impl Default for EnvelopeState {
    fn default() -> Self {
        Self {
            phase: ADSRPhase::Attack,
            elapsed: 0.0,
            gate_open: true,
        }
    }
}

impl EnvelopeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trigger(&mut self) {
        self.phase = ADSRPhase::Attack;
        self.elapsed = 0.0;
        self.gate_open = true;
    }

    pub fn release(&mut self) {
        if self.phase != ADSRPhase::Done {
            self.phase = ADSRPhase::Release;
            self.elapsed = 0.0;
            self.gate_open = false;
        }
    }

    /// Calculate envelope gain for current sample
    pub fn get_gain(&mut self, adsr: &ADSREnvelope, sample_rate: f32) -> f32 {
        if self.phase == ADSRPhase::Done {
            return 0.0;
        }

        let dt = 1.0 / sample_rate as f64;
        self.elapsed += dt;

        match self.phase {
            ADSRPhase::Attack => {
                if adsr.attack <= 0.0 {
                    self.phase = ADSRPhase::Decay;
                    return 1.0;
                }
                let gain = (self.elapsed / adsr.attack as f64).min(1.0) as f32;
                if gain >= 1.0 {
                    self.phase = ADSRPhase::Decay;
                    self.elapsed = 0.0;
                }
                gain
            }
            ADSRPhase::Decay => {
                if adsr.decay <= 0.0 {
                    self.phase = ADSRPhase::Sustain;
                    return adsr.sustain;
                }
                let decay_progress = (self.elapsed / adsr.decay as f64).min(1.0);
                let gain = 1.0 - (1.0 - adsr.sustain) * decay_progress as f32;
                if decay_progress >= 1.0 {
                    self.phase = ADSRPhase::Sustain;
                    self.elapsed = 0.0;
                }
                gain
            }
            ADSRPhase::Sustain => {
                if !self.gate_open {
                    self.phase = ADSRPhase::Release;
                    self.elapsed = 0.0;
                    return adsr.sustain;
                }
                adsr.sustain
            }
            ADSRPhase::Release => {
                if adsr.release <= 0.0 {
                    self.phase = ADSRPhase::Done;
                    return 0.0;
                }
                let release_progress = (self.elapsed / adsr.release as f64).min(1.0);
                let gain = adsr.sustain * (1.0 - release_progress as f32);
                if release_progress >= 1.0 {
                    self.phase = ADSRPhase::Done;
                }
                gain
            }
            ADSRPhase::Done => 0.0,
        }
    }

    pub fn is_done(&self) -> bool {
        self.phase == ADSRPhase::Done
    }
}

/// Voice with PCM data and envelope
#[derive(Clone)]
pub struct Voice {
    pub pcm: Arc<Vec<f32>>,
    pub channels: usize,
    pub start_frame: usize,
    pub frame_pos: f64,
    pub speed: f32,
    pub adsr: ADSREnvelope,
    pub envelope: EnvelopeState,
}

impl Voice {
    pub fn new(
        pcm: Arc<Vec<f32>>,
        channels: usize,
        start_frame: usize,
        speed: f32,
        adsr: ADSREnvelope,
    ) -> Self {
        Self {
            pcm,
            channels,
            start_frame,
            frame_pos: start_frame as f64,
            speed,
            adsr,
            envelope: EnvelopeState::new(),
        }
    }

    pub fn trigger(&mut self) {
        self.envelope.trigger();
    }

    pub fn release(&mut self) {
        self.envelope.release();
    }

    /// Render one sample frame, returns gain-adjusted sample
    pub fn render(&mut self, sample_rate: f32, out_channels: usize) -> Option<Vec<f32>> {
        if self.envelope.is_done() {
            return None;
        }

        let pcm_frames = self.pcm.len() / self.channels.max(1);
        let i0 = self.frame_pos as usize;

        if i0 >= pcm_frames.saturating_sub(1) {
            if self.envelope.gate_open {
                self.envelope.release();
            }
            if self.envelope.is_done() {
                return None;
            }
        }

        let i1 = (i0 + 1).min(pcm_frames - 1);
        let t = (self.frame_pos - i0 as f64) as f32;
        let gain = self.envelope.get_gain(&self.adsr, sample_rate);

        let mut samples = Vec::with_capacity(out_channels);
        for oc in 0..out_channels {
            let sc = oc.min(self.channels - 1);
            let s0 = self.pcm.get(i0 * self.channels + sc).copied().unwrap_or(0.0);
            let s1 = self.pcm.get(i1 * self.channels + sc).copied().unwrap_or(0.0);
            let smp = (s0 + t * (s1 - s0)) * gain;
            samples.push(smp);
        }

        self.frame_pos += self.speed as f64;
        Some(samples)
    }

    pub fn is_finished(&self) -> bool {
        self.envelope.is_done()
    }
}