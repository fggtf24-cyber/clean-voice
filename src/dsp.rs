//! DSP core: wraps DeepFilterNet3 (libDF / tract) for arbitrary host sample rates and block sizes.
//!
//! Signal flow per channel (host rate `sr`, model rate 48 kHz):
//!   in -> [chunk FIFO, C samples] -> resample sr->48k -> [pad delay E] -> DF3 (hop 480)
//!      -> resample 48k->sr -> out
//! Everything works on fixed chunks, so the total delay is a constant integer number of host
//! samples, independent of the host block size. `E` is chosen so that the 48 kHz part of the delay
//! maps to a whole number of host samples (needed for sample-accurate sync at 44.1 kHz etc.).

use crate::dereverb::{self, Dereverb};
use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};
use rubato::{FftFixedInOut, Resampler};

pub const MODEL_SR: usize = 48_000;

/// Runtime settings (all cheap to change per block).
#[derive(Clone, Copy)]
pub struct Settings {
    /// 0..100 dB, 100 = unlimited noise reduction.
    pub atten_db: f32,
    /// Post-filter beta, 0 = off.
    pub pf_beta: f32,
    /// Stereo: process the mono sum once and feed both channels.
    pub link: bool,
    /// 50/60 = remove mains hum + harmonics (also from the dry path), 0 = off.
    pub hum_hz: f32,
    /// Dereverb amount 0..1.
    pub dereverb: f32,
    /// Assumed room reverb time (T60, seconds) for the dereverb.
    pub room_t60: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { atten_db: 100.0, pf_beta: 0.0, link: false, hum_hz: 0.0, dereverb: 0.0, room_t60: 0.6 }
    }
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Fixed-length delay line (length 0 = passthrough).
struct Delay {
    buf: Vec<f32>,
    pos: usize,
}

impl Delay {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], pos: 0 }
    }
    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        if self.buf.is_empty() {
            return x;
        }
        let y = std::mem::replace(&mut self.buf[self.pos], x);
        self.pos = (self.pos + 1) % self.buf.len();
        y
    }
    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }
}

const HUM_HARMONICS: usize = 24;
const HUM_BW_HZ: f64 = 3.0;

/// Mains hum remover: narrow notches at f0 and its harmonics (TDF-II biquads, f64 for the
/// very high-Q low-frequency poles). No latency.
struct Hum {
    sr: f64,
    f0: f32,
    n: usize,
    c: [[f64; 5]; HUM_HARMONICS],
    z: [[[f64; 2]; HUM_HARMONICS]; 2],
}

impl Hum {
    fn new(sr: usize) -> Self {
        Self { sr: sr as f64, f0: 0.0, n: 0, c: [[0.0; 5]; HUM_HARMONICS], z: [[[0.0; 2]; HUM_HARMONICS]; 2] }
    }
    /// `f0` = 50 or 60 Hz, 0 = off. Alloc-free.
    fn set(&mut self, f0: f32) {
        if f0 == self.f0 {
            return;
        }
        self.f0 = f0;
        self.n = 0;
        if f0 > 0.0 {
            for k in 1..=HUM_HARMONICS {
                let f = f0 as f64 * k as f64;
                if f >= self.sr * 0.45 {
                    break;
                }
                let w = 2.0 * std::f64::consts::PI * f / self.sr;
                let alpha = w.sin() / (2.0 * (f / HUM_BW_HZ));
                let (a0, cw) = (1.0 + alpha, w.cos());
                self.c[self.n] = [1.0 / a0, -2.0 * cw / a0, 1.0 / a0, -2.0 * cw / a0, (1.0 - alpha) / a0];
                self.n += 1;
            }
        }
        self.reset();
    }
    fn reset(&mut self) {
        self.z = [[[0.0; 2]; HUM_HARMONICS]; 2];
    }
    #[inline]
    fn tick(&mut self, ch: usize, x: f32) -> f32 {
        let mut y = x as f64;
        for i in 0..self.n {
            let [b0, b1, b2, a1, a2] = self.c[i];
            let z = &mut self.z[ch][i];
            let o = b0 * y + z[0];
            z[0] = b1 * y - a1 * o + z[1];
            z[1] = b2 * y - a2 * o;
            y = o;
        }
        y as f32
    }
}

pub struct Engine {
    n_ch: usize,
    hum: Hum,
    host_sr: usize,
    /// Host-rate chunk size (C) and 48 kHz chunk size (M = k * hop).
    chunk_host: usize,
    chunk_model: usize,
    hop: usize,
    latency: usize,

    models: Vec<DfTract>,
    pristine: DfTract,
    rs_in: Option<FftFixedInOut<f32>>,
    rs_out: Option<FftFixedInOut<f32>>,
    pad: Vec<Delay>,
    dry_delay: Vec<Delay>,

    pos: usize,
    in_host: Vec<Vec<f32>>,
    out_host: Vec<Vec<f32>>,
    buf_model_in: Vec<Vec<f32>>,
    buf_model_out: Vec<Vec<f32>>,

    link: bool,
    derev: Vec<Dereverb>,
    derev_amount: f32,
    room_t60: f32,
    /// Current noise reduction (dB) measured over the last chunk.
    pub reduction_db: f32,
    e_dry: f32,
    e_wet: f32,
}

// SAFETY: DfTract holds `Rc`s (tract tensors) and is therefore !Send. Every Rc alias (including
// those shared between `pristine` and `models` via clone) lives inside this one Engine, which is
// only ever moved between threads as a whole and never shared (no Sync), so this is sound.
unsafe impl Send for Engine {}

impl Engine {
    /// Allocates everything. Call from `initialize()` only, never from the audio thread.
    pub fn new(host_sr: usize, n_ch: usize) -> Self {
        assert!((1..=2).contains(&n_ch));
        // Same LSNR thresholds as the official `deep-filter` CLI (library defaults differ).
        let rp = RuntimeParams::default_with_ch(1).with_thresholds(-15.0, 35.0, 35.0);
        let pristine = DfTract::new(DfParams::default(), &rp).expect("failed to load DeepFilterNet3");
        let hop = pristine.hop_size;
        assert_eq!(pristine.sr, MODEL_SR);
        // DF3 STFT + lookahead, plus the dereverb STFT (always running => constant latency).
        let model_delay = pristine.fft_size - hop + pristine.lookahead * hop + (dereverb::FFT - dereverb::HOP);
        assert_eq!(hop, dereverb::HOP);

        // Smallest number of hops k so that k*hop*host_sr/48000 is an integer.
        let k = (MODEL_SR / gcd(hop * host_sr, MODEL_SR)).max(1);
        let chunk_model = k * hop;
        let chunk_host = chunk_model * host_sr / MODEL_SR;

        let (rs_in, rs_out, d_in, d_out) = if host_sr == MODEL_SR {
            (None, None, 0, 0)
        } else {
            let rin = FftFixedInOut::<f32>::new(host_sr, MODEL_SR, chunk_model, n_ch).unwrap();
            let rout = FftFixedInOut::<f32>::new(MODEL_SR, host_sr, chunk_host, n_ch).unwrap();
            assert_eq!((rin.input_frames_next(), rin.output_frames_next()), (chunk_host, chunk_model));
            assert_eq!((rout.input_frames_next(), rout.output_frames_next()), (chunk_model, chunk_host));
            let (di, d_o) = (rin.output_delay(), rout.output_delay());
            (Some(rin), Some(rout), di, d_o)
        };

        // Pad the 48k-domain delay so it converts to an exact number of host samples.
        let b = MODEL_SR / gcd(host_sr, MODEL_SR);
        let pad_len = (b - (d_in + model_delay) % b) % b;
        let delay48 = d_in + pad_len + model_delay;
        debug_assert_eq!(delay48 * host_sr % MODEL_SR, 0);
        let latency = chunk_host + delay48 * host_sr / MODEL_SR + d_out;

        Self {
            n_ch,
            hum: Hum::new(host_sr),
            host_sr,
            chunk_host,
            chunk_model,
            hop,
            latency,
            models: (0..n_ch).map(|_| pristine.clone()).collect(),
            pristine,
            rs_in,
            rs_out,
            pad: (0..n_ch).map(|_| Delay::new(pad_len)).collect(),
            dry_delay: (0..n_ch).map(|_| Delay::new(latency)).collect(),
            pos: 0,
            in_host: vec![vec![0.0; chunk_host]; n_ch],
            out_host: vec![vec![0.0; chunk_host]; n_ch],
            buf_model_in: vec![vec![0.0; chunk_model]; n_ch],
            buf_model_out: vec![vec![0.0; chunk_model]; n_ch],
            link: false,
            derev: (0..n_ch).map(|_| Dereverb::new()).collect(),
            derev_amount: 0.0,
            room_t60: 0.6,
            reduction_db: 0.0,
            e_dry: 0.0,
            e_wet: 0.0,
        }
    }

    /// Total plugin latency in host samples (report via `set_latency_samples`).
    pub fn latency(&self) -> usize {
        self.latency
    }

    pub fn channels(&self) -> usize {
        self.n_ch
    }

    pub fn sample_rate(&self) -> usize {
        self.host_sr
    }

    /// Restores the freshly-loaded model state and clears all buffers.
    // ponytail: cloning the pristine model allocates a little; tract has no in-place, alloc-free
    // state reset. Fine for transport start/seek; upgrade = patch libDF with a zeroing reset.
    pub fn reset(&mut self) {
        for m in &mut self.models {
            m.clone_from(&self.pristine);
        }
        self.pristine_params_sync();
        if let Some(r) = &mut self.rs_in { r.reset(); }
        if let Some(r) = &mut self.rs_out { r.reset(); }
        self.pad.iter_mut().for_each(Delay::reset);
        self.dry_delay.iter_mut().for_each(Delay::reset);
        self.hum.reset();
        self.derev.iter_mut().for_each(Dereverb::reset);
        self.in_host.iter_mut().chain(&mut self.out_host).for_each(|b| b.fill(0.0));
        self.pos = 0;
        self.reduction_db = 0.0;
    }

    // Keeps runtime params (set via set_params) after a reset clone.
    fn pristine_params_sync(&mut self) {
        let (lim, pf, beta) = (self.pristine.atten_lim, self.pristine.post_filter, self.pristine.post_filter_beta);
        for m in &mut self.models {
            m.atten_lim = lim;
            m.post_filter = pf;
            m.post_filter_beta = beta;
        }
    }

    pub fn set_params(&mut self, st: Settings) {
        let Settings { atten_db, pf_beta, link, hum_hz, dereverb, room_t60 } = st;
        self.hum.set(hum_hz);
        self.derev_amount = dereverb.clamp(0.0, 1.0);
        self.room_t60 = room_t60;
        // libDF passes the *undelayed* input through at exactly 0 dB, which would break sync,
        // so never go below 0.01 dB (inaudible difference).
        let atten_db = atten_db.max(0.01);
        self.pristine.set_atten_lim(atten_db);
        self.pristine.set_pf_beta(pf_beta.max(0.0));
        self.pristine_params_sync();
        if self.n_ch == 2 && self.link && !link {
            // Channel 2 model was idle while linked: start it from a clean state.
            let m = &mut self.models[1];
            m.clone_from(&self.pristine);
        }
        self.link = link && self.n_ch == 2;
    }

    /// Processes one sample frame. `x`, `wet` and `dry` have `channels()` entries.
    /// `dry` is the input delayed by `latency()` (aligned with `wet`).
    #[inline]
    pub fn tick(&mut self, x: &[f32], wet: &mut [f32], dry: &mut [f32]) {
        let p = self.pos;
        for c in 0..self.n_ch {
            let xc = self.hum.tick(c, x[c]);
            self.in_host[c][p] = xc;
            wet[c] = self.out_host[c][p];
            dry[c] = self.dry_delay[c].tick(xc);
            self.e_dry += dry[c] * dry[c];
            self.e_wet += wet[c] * wet[c];
        }
        self.pos += 1;
        if self.pos == self.chunk_host {
            self.pos = 0;
            self.process_chunk();
            let r = 10.0 * ((self.e_dry + 1e-9) / (self.e_wet + 1e-9)).log10();
            self.reduction_db = r.max(0.0);
            self.e_dry = 0.0;
            self.e_wet = 0.0;
        }
    }

    fn process_chunk(&mut self) {
        // host -> 48k
        match &mut self.rs_in {
            Some(r) => {
                r.process_into_buffer(&self.in_host, &mut self.buf_model_in, None).unwrap();
            }
            None => {
                for (d, s) in self.buf_model_in.iter_mut().zip(&self.in_host) {
                    d.copy_from_slice(s);
                }
            }
        }
        let n_models = if self.link {
            let (a, b) = self.buf_model_in.split_at_mut(1);
            for (l, r) in a[0].iter_mut().zip(b[0].iter()) {
                *l = 0.5 * (*l + *r);
            }
            1
        } else {
            self.n_ch
        };
        for c in 0..n_models {
            for s in self.buf_model_in[c].iter_mut() {
                *s = self.pad[c].tick(*s);
            }
            for h in 0..self.chunk_model / self.hop {
                let r = h * self.hop..(h + 1) * self.hop;
                let inp = ArrayView2::from_shape((1, self.hop), &self.buf_model_in[c][r.clone()]).unwrap();
                let out = ArrayViewMut2::from_shape((1, self.hop), &mut self.buf_model_out[c][r]).unwrap();
                // ponytail: tract allocates internally during inference; our code around it doesn't.
                // Removing that would need a forked tract — not worth it, runtime cost is tiny.
                let _ = self.models[c].process(inp, out);
                let r = h * self.hop..(h + 1) * self.hop;
                self.derev[c].process(&mut self.buf_model_out[c][r], self.derev_amount, self.room_t60);
            }
        }
        if self.link {
            let (a, b) = self.buf_model_out.split_at_mut(1);
            b[0].copy_from_slice(&a[0]);
        }
        // 48k -> host
        match &mut self.rs_out {
            Some(r) => {
                r.process_into_buffer(&self.buf_model_out, &mut self.out_host, None).unwrap();
            }
            None => {
                for (d, s) in self.out_host.iter_mut().zip(&self.buf_model_out) {
                    d.copy_from_slice(s);
                }
            }
        }
    }
}
