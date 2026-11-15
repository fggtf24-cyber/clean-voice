//! Runs a WAV file through the plugin's DSP core (latency-compensated).
//! Usage: process_file <in.wav> <out.wav> [strength_db=100] [hum_hz=0] [pf_beta=0] [dereverb=0..1] [room_t60=0.6]
use clean_voice::dsp::Engine;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let arg = |i: usize, d: f32| a.get(i).map_or(d, |s| s.parse().unwrap());
    let mut r = hound::WavReader::open(&a[1]).unwrap();
    let spec = r.spec();
    let ch = spec.channels as usize;
    let x: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(Result::unwrap).collect(),
        hound::SampleFormat::Int => {
            let k = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.unwrap() as f32 * k).collect()
        }
    };
    assert!(ch <= 2, "mono or stereo only");
    let mut e = Engine::new(spec.sample_rate as usize, ch);
    e.set_params(clean_voice::dsp::Settings {
        atten_db: arg(3, 100.0),
        hum_hz: arg(4, 0.0),
        pf_beta: arg(5, 0.0),
        dereverb: arg(6, 0.0),
        room_t60: arg(7, 0.6),
        ..Default::default()
    });
    let (lat, frames) = (e.latency(), x.len() / ch);
    let (mut w, mut d) = ([0f32; 2], [0f32; 2]);
    let mut out = Vec::with_capacity(x.len());
    for i in 0..frames + lat {
        let f: Vec<f32> = (0..ch).map(|c| if i < frames { x[i * ch + c] } else { 0.0 }).collect();
        e.tick(&f, &mut w[..ch], &mut d[..ch]);
        if i >= lat {
            out.extend_from_slice(&w[..ch]);
        }
    }
    let os = hound::WavSpec { bits_per_sample: 32, sample_format: hound::SampleFormat::Float, ..spec };
    let mut wr = hound::WavWriter::create(&a[2], os).unwrap();
    out.iter().for_each(|&s| wr.write_sample(s).unwrap());
    wr.finalize().unwrap();
    println!("done: {} frames, latency {} samples compensated", frames, lat);
}
