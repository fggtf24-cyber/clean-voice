//! Late-reverberation suppression (statistical model, Lebart 2001 / Habets 2009).
//!
//! The late reverb PSD in frame t is predicted from the observed PSD `ND` frames earlier, decayed
//! according to the room's T60: λ(t,k) = exp(-2·Δ·ND·hop/sr) · P(t-ND,k), Δ = 3·ln10 / T60.
//! A Wiener-like gain then removes it. Direct sound + early reflections (< ND·10 ms) are kept.
//! Runs at 48 kHz on 480-sample hops, sqrt-Hann WOLA => exactly 480 samples of latency.

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use std::sync::Arc;

pub const FFT: usize = 960;
pub const HOP: usize = 480;
const BINS: usize = FFT / 2 + 1;
const SR: f32 = 48_000.0;
/// Late reverb starts ~50 ms after the direct sound.
const ND: usize = 5;

pub struct Dereverb {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    win: Vec<f32>,
    prev: Vec<f32>,
    ola: Vec<f32>,
    frame: Vec<f32>,
    spec: Vec<Complex32>,
    scratch_f: Vec<Complex32>,
    scratch_i: Vec<Complex32>,
    psd: Vec<f32>,
    hist: Vec<Vec<f32>>,
    hpos: usize,
    gain: Vec<f32>,
}

impl Dereverb {
    pub fn new() -> Self {
        let mut pl = RealFftPlanner::<f32>::new();
        let (fwd, inv) = (pl.plan_fft_forward(FFT), pl.plan_fft_inverse(FFT));
        let win = (0..FFT)
            .map(|n| (0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / FFT as f32).cos()).sqrt())
            .collect();
        Self {
            scratch_f: fwd.make_scratch_vec(),
            scratch_i: inv.make_scratch_vec(),
            fwd,
            inv,
            win,
            prev: vec![0.0; HOP],
            ola: vec![0.0; HOP],
            frame: vec![0.0; FFT],
            spec: vec![Complex32::default(); BINS],
            psd: vec![0.0; BINS],
            hist: vec![vec![0.0; BINS]; ND],
            hpos: 0,
            gain: vec![1.0; BINS],
        }
    }

    pub fn reset(&mut self) {
        self.prev.fill(0.0);
        self.ola.fill(0.0);
        self.psd.fill(0.0);
        self.hist.iter_mut().for_each(|h| h.fill(0.0));
        self.hpos = 0;
        self.gain.fill(1.0);
    }

    /// Processes one hop in place (output is delayed by `HOP`). `amount` 0..1, `t60` in seconds.
    /// Alloc-free.
    pub fn process(&mut self, io: &mut [f32], amount: f32, t60: f32) {
        debug_assert_eq!(io.len(), HOP);
        self.frame[..HOP].copy_from_slice(&self.prev);
        self.frame[HOP..].copy_from_slice(io);
        self.prev.copy_from_slice(io);
        for (s, w) in self.frame.iter_mut().zip(&self.win) {
            *s *= w;
        }
        let _ = self.fwd.process_with_scratch(&mut self.frame, &mut self.spec, &mut self.scratch_f);

        // Energy decay over ND hops for the given T60 (60 dB per T60).
        let decay = 10f32.powf(-6.0 * (ND * HOP) as f32 / (SR * t60.max(0.05)));
        let alpha = 2.0 * amount; // over-subtraction
        let gmin = 10f32.powf(-25.0 * amount / 20.0);
        let old = &mut self.hist[self.hpos];
        for k in 0..BINS {
            let p = self.spec[k].norm_sqr();
            self.psd[k] = 0.6 * self.psd[k] + 0.4 * p;
            let late = decay * old[k];
            let g = (1.0 - alpha * late / (self.psd[k] + 1e-12)).max(gmin);
            // Smooth the gain over time to avoid musical noise.
            self.gain[k] = 0.5 * self.gain[k] + 0.5 * g;
            if amount > 0.0 {
                self.spec[k] *= self.gain[k];
            }
            old[k] = self.psd[k];
        }
        self.hpos = (self.hpos + 1) % ND;
        self.spec[0].im = 0.0;
        self.spec[BINS - 1].im = 0.0;
        let _ = self.inv.process_with_scratch(&mut self.spec, &mut self.frame, &mut self.scratch_i);

        let norm = 1.0 / FFT as f32;
        for i in 0..HOP {
            io[i] = self.ola[i] + self.frame[i] * self.win[i] * norm;
            self.ola[i] = self.frame[HOP + i] * self.win[HOP + i] * norm;
        }
    }
}
