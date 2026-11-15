mod dereverb;
pub mod dsp;
mod editor;
mod native;
mod update;

use dsp::Engine;
use nih_plug::prelude::*;
use std::num::NonZeroU32;
use std::sync::Arc;

#[derive(Params)]
struct CvParams {
    // Not persisted on purpose: an old saved size would crop the window after UI updates.
    editor_state: Arc<nih_plug_egui::EguiState>,
    /// Selected preset: "#<n>" = factory preset n, otherwise a user preset name.
    #[persist = "preset"]
    preset: Arc<std::sync::RwLock<String>>,
    #[id = "strength"]
    strength: FloatParam,
    #[id = "mix"]
    mix: FloatParam,
    #[id = "pf"]
    post_filter: BoolParam,
    #[id = "pf_beta"]
    pf_beta: FloatParam,
    #[id = "delta"]
    delta: BoolParam,
    #[id = "gain"]
    gain: FloatParam,
    #[id = "link"]
    link: BoolParam,
    #[id = "dereverb"]
    dereverb: FloatParam,
    #[id = "room"]
    room: FloatParam,
    #[id = "hum"]
    hum: EnumParam<HumMode>,
    #[id = "bypass"]
    bypass: BoolParam,
}

#[derive(Enum, PartialEq, Clone, Copy)]
pub enum HumMode {
    Off,
    #[name = "50 Hz"]
    Hz50,
    #[name = "60 Hz"]
    Hz60,
}

impl HumMode {
    fn hz(self) -> f32 {
        match self {
            HumMode::Off => 0.0,
            HumMode::Hz50 => 50.0,
            HumMode::Hz60 => 60.0,
        }
    }
}

impl Default for CvParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_state(),
            preset: Arc::new(std::sync::RwLock::new("#0".into())),
            strength: FloatParam::new("Strength", 100.0, FloatRange::Linear { min: 0.0, max: 100.0 })
                .with_unit(" dB")
                .with_step_size(0.1),
            mix: FloatParam::new("Mix", 1.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            post_filter: BoolParam::new("Post-filter", false),
            pf_beta: FloatParam::new("Post-filter Strength", 0.02, FloatRange::Linear { min: 0.0, max: 0.05 })
                .with_step_size(0.001),
            delta: BoolParam::new("Delta", false),
            gain: FloatParam::new(
                "Output",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-12.0),
                    max: util::db_to_gain(12.0),
                    factor: FloatRange::gain_skew_factor(-12.0, 12.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            link: BoolParam::new("Link (mono)", false),
            dereverb: FloatParam::new("Dereverb", 0.0, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
            room: FloatParam::new("Room Size", 0.6, FloatRange::Skewed { min: 0.2, max: 2.0, factor: FloatRange::skew_factor(-1.0) })
                .with_unit(" s")
                .with_value_to_string(formatters::v2s_f32_rounded(2)),
            hum: EnumParam::new("Hum Removal", HumMode::Off),
            bypass: BoolParam::new("Bypass", false).make_bypass(),
        }
    }
}

pub struct CleanVoice {
    params: Arc<CvParams>,
    engine: Option<Engine>,
    reduction: Arc<AtomicF32>,
    bypass_mix: Smoother<f32>,
}

impl Default for CleanVoice {
    fn default() -> Self {
        Self { params: Arc::new(CvParams::default()), engine: None, reduction: Arc::new(AtomicF32::new(0.0)), bypass_mix: Smoother::new(SmoothingStyle::Linear(10.0)) }
    }
}

impl Plugin for CleanVoice {
    const NAME: &'static str = "Clean Voice";
    const VENDOR: &'static str = "Clean Voice";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone(), self.reduction.clone())
    }

    fn initialize(&mut self, layout: &AudioIOLayout, cfg: &BufferConfig, ctx: &mut impl InitContext<Self>) -> bool {
        let ch = layout.main_input_channels.map_or(2, |c| c.get()) as usize;
        let sr = cfg.sample_rate.round() as usize;
        // Rebuild only when the config actually changed (model load takes a moment).
        if self.engine.as_ref().map_or(true, |e| e.channels() != ch || e.sample_rate() != sr) {
            self.engine = Some(Engine::new(sr, ch));
        }
        ctx.set_latency_samples(self.engine.as_ref().unwrap().latency() as u32);
        true
    }

    fn reset(&mut self) {
        if let Some(e) = &mut self.engine {
            e.reset();
        }
        self.bypass_mix.reset(if self.params.bypass.value() { 1.0 } else { 0.0 });
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, ctx: &mut impl ProcessContext<Self>) -> ProcessStatus {
        let Some(e) = self.engine.as_mut() else { return ProcessStatus::Normal };
        let p = &self.params;
        let pf = if p.post_filter.value() { p.pf_beta.value() } else { 0.0 };
        e.set_params(dsp::Settings {
            atten_db: p.strength.value(),
            pf_beta: pf,
            link: p.link.value(),
            hum_hz: p.hum.value().hz(),
            dereverb: p.dereverb.value(),
            room_t60: p.room.value(),
        });
        let delta = p.delta.value();
        let sr = ctx.transport().sample_rate;
        self.bypass_mix.set_target(sr, if p.bypass.value() { 1.0 } else { 0.0 });

        let (mut x, mut w, mut d) = ([0f32; 2], [0f32; 2], [0f32; 2]);
        let n = e.channels();
        for mut frame in buffer.iter_samples() {
            let mix = p.mix.smoothed.next();
            let gain = p.gain.smoothed.next();
            let byp = self.bypass_mix.next();
            for (c, s) in frame.iter_mut().enumerate().take(n) {
                x[c] = *s;
            }
            e.tick(&x[..n], &mut w[..n], &mut d[..n]);
            for (c, s) in frame.iter_mut().enumerate().take(n) {
                let m = d[c] + mix * (w[c] - d[c]);
                let o = if delta { d[c] - m } else { m } * gain;
                // Bypass still outputs the delayed dry signal so latency stays constant.
                *s = o + byp * (d[c] - o);
            }
        }
        let r = if p.bypass.value() { 0.0 } else { e.reduction_db };
        self.reduction.store(r, std::sync::atomic::Ordering::Relaxed);
        ProcessStatus::Normal
    }
}

impl ClapPlugin for CleanVoice {
    const CLAP_ID: &'static str = "local.clean-voice.df3";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("DeepFilterNet3 voice noise reduction");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::AudioEffect, ClapFeature::Restoration, ClapFeature::Stereo, ClapFeature::Mono];
}

impl Vst3Plugin for CleanVoice {
    const VST3_CLASS_ID: [u8; 16] = *b"CleanVoiceDF3v01";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Restoration];
}

nih_export_clap!(CleanVoice);
nih_export_vst3!(CleanVoice);
