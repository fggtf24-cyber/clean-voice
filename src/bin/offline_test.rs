//! Offline self-check of the DSP core against the official `deep-filter -D` CLI.
//! Usage: cargo run --release --bin offline_test -- <deep-filter.exe> <clean.wav> <noise.wav> <out_dir>
use clean_voice::dsp::Engine;
use std::{path::Path, process::Command, time::Instant};

fn read_mono(p: &Path) -> (Vec<f32>, usize) {
    let mut r = hound::WavReader::open(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let s = r.spec();
    let ch = s.channels as usize;
    let all: Vec<f32> = match s.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(|x| x.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let k = 1.0 / (1u64 << (s.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|x| x.unwrap() as f32 * k).collect()
        }
    };
    (all.chunks(ch).map(|f| f[0]).collect(), s.sample_rate as usize)
}

fn write_mono(p: &Path, x: &[f32], sr: usize) {
    let spec = hound::WavSpec { channels: 1, sample_rate: sr as u32, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(p, spec).unwrap();
    x.iter().for_each(|&s| w.write_sample(s).unwrap());
    w.finalize().unwrap();
}

fn resample(x: &[f32], from: usize, to: usize) -> Vec<f32> {
    if from == to {
        return x.to_vec();
    }
    let a = ndarray::ArrayView2::from_shape((1, x.len()), x).unwrap();
    df::transforms::resample(a, from, to, None).unwrap().row(0).to_vec()
}

/// Runs the engine like a host would: fixed block size, then removes the reported latency.
fn run(e: &mut Engine, input: &[Vec<f32>], block: usize) -> Vec<Vec<f32>> {
    let (n, ch, lat) = (input[0].len(), e.channels(), e.latency());
    let mut out = vec![Vec::with_capacity(n + lat); ch];
    let (mut x, mut w, mut d) = ([0f32; 2], [0f32; 2], [0f32; 2]);
    let total = n + lat;
    let mut i = 0;
    while i < total {
        for j in i..(i + block).min(total) {
            for c in 0..ch {
                x[c] = if j < n { input[c][j] } else { 0.0 };
            }
            e.tick(&x[..ch], &mut w[..ch], &mut d[..ch]);
            for c in 0..ch {
                out[c].push(w[c]);
            }
        }
        i += block;
    }
    out.into_iter().map(|v| v[lat..].to_vec()).collect()
}

/// Lag (in samples, with parabolic sub-sample refinement) that maximises xcorr(a, b).
/// Positive = `b` is late relative to `a`.
fn xcorr_lag(a: &[f32], b: &[f32], max_lag: i64) -> f64 {
    let n = a.len().min(b.len()) as i64;
    let c = |lag: i64| -> f64 {
        let mut s = 0.0f64;
        for i in max_lag..n - max_lag {
            s += a[i as usize] as f64 * b[(i + lag) as usize] as f64;
        }
        s
    };
    let vals: Vec<f64> = (-max_lag..=max_lag).map(c).collect();
    let k = (0..vals.len()).max_by(|&i, &j| vals[i].total_cmp(&vals[j])).unwrap();
    let mut frac = 0.0;
    if k > 0 && k + 1 < vals.len() {
        let (y0, y1, y2) = (vals[k - 1], vals[k], vals[k + 1]);
        let den = y0 - 2.0 * y1 + y2;
        if den != 0.0 {
            frac = 0.5 * (y0 - y2) / den;
        }
    }
    (k as i64 - max_lag) as f64 + frac
}

fn snr_db(reference: &[f32], test: &[f32]) -> f64 {
    let n = reference.len().min(test.len());
    let (mut s, mut e) = (0f64, 0f64);
    for i in 0..n {
        s += (reference[i] as f64).powi(2);
        e += ((reference[i] - test[i]) as f64).powi(2);
    }
    10.0 * (s / e.max(1e-30)).log10()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (cli, clean_p, noise_p, out_dir) = (Path::new(&a[1]), Path::new(&a[2]), Path::new(&a[3]), Path::new(&a[4]));
    std::fs::create_dir_all(out_dir).unwrap();

    let (clean, sr0) = read_mono(clean_p);
    let (noise, sr1) = read_mono(noise_p);
    let noise = resample(&noise, sr1, sr0);
    let clean = resample(&clean, sr0, 48000);
    let noise = resample(&noise, sr0, 48000);
    // Noisy mix at ~5 dB SNR.
    let n = clean.len().min(noise.len());
    let pc: f32 = clean[..n].iter().map(|x| x * x).sum();
    let pn: f32 = noise[..n].iter().map(|x| x * x).sum();
    let g = (pc / pn / 10f32.powf(0.5)).sqrt();
    let noisy48: Vec<f32> = (0..n).map(|i| 0.7 * (clean[i] + g * noise[i])).collect();
    let clean48: Vec<f32> = clean[..n].iter().map(|x| 0.7 * x).collect();
    let mut ok = true;

    for sr in [48000usize, 44100] {
        let noisy = resample(&noisy48, 48000, sr);
        let clean_ref = resample(&clean48, 48000, sr);
        let in_path = out_dir.join(format!("noisy_{sr}.wav"));
        write_mono(&in_path, &noisy, sr);
        let cli_dir = out_dir.join(format!("cli_{sr}"));
        let st = Command::new(cli).arg("-D").arg("-o").arg(&cli_dir).arg(&in_path).status().unwrap();
        assert!(st.success(), "deep-filter CLI failed");
        let (cli_out, _) = read_mono(&cli_dir.join(in_path.file_name().unwrap()));

        println!("  (reference) CLI vs clean speech lag = {:+.2}", xcorr_lag(&clean_ref, &cli_out, 400));
        let mut e = Engine::new(sr, 1);
        println!("\n=== {sr} Hz: reported latency = {} samples ({:.2} ms)", e.latency(), e.latency() as f64 * 1000.0 / sr as f64);
        let mut first: Option<Vec<f32>> = None;
        for bs in [64usize, 512, 2048] {
            e.reset();
            let y = run(&mut e, &[noisy.clone()], bs).remove(0);
            write_mono(&out_dir.join(format!("plugin_{sr}_bs{bs}.wav")), &y, sr);
            let lag_cli = xcorr_lag(&cli_out, &y, 400);
            let lag_clean = xcorr_lag(&clean_ref, &y, 400);
            let snr_cli = snr_db(&cli_out, &y);
            let same = first.as_ref().map(|f| f == &y);
            println!(
                "  block {bs:>4}: lag vs CLI = {lag_cli:+.2} | lag vs clean speech = {lag_clean:+.2} | match CLI = {snr_cli:.1} dB SNR | identical to block 64: {}",
                same.map_or("-".into(), |s| s.to_string())
            );
            ok &= lag_cli.abs() < 0.5 && lag_clean.abs() < 0.5 && same.unwrap_or(true);
            if sr == 48000 {
                ok &= snr_cli > 40.0;
            }
            first.get_or_insert(y);
        }
    }

    // Stereo: independent channels must equal the mono result; Link mode must run.
    let mut em = Engine::new(48000, 1);
    let mono = run(&mut em, &[noisy48.clone()], 512).remove(0);
    let mut es = Engine::new(48000, 2);
    let st = run(&mut es, &[noisy48.clone(), noisy48.clone()], 512);
    let stereo_ok = st[0] == mono && st[1] == mono;
    es.reset();
    es.set_params(clean_voice::dsp::Settings { link: true, ..Default::default() });
    let linked = run(&mut es, &[noisy48.clone(), noisy48.clone()], 512);
    let link_ok = snr_db(&mono, &linked[0]) > 40.0 && linked[0] == linked[1];
    println!("\nstereo L/R == mono: {stereo_ok} | link mode == mono: {link_ok}");
    ok &= stereo_ok && link_ok;

    // CPU: stereo 48 kHz, ~60 s of audio.
    let reps = (60 * 48000 / noisy48.len()).max(1);
    let long: Vec<f32> = noisy48.iter().cycle().take(reps * noisy48.len()).copied().collect();
    let mut ec = Engine::new(48000, 2);
    let t = Instant::now();
    run(&mut ec, &[long.clone(), long.clone()], 512);
    let el = t.elapsed().as_secs_f64();
    let dur = long.len() as f64 / 48000.0;
    println!("CPU: {:.1}% of one core (stereo 48 kHz, {:.0} s audio in {:.2} s)", el / dur * 100.0, dur, el);

    println!("\nRESULT: {}", if ok { "PASS" } else { "FAIL" });
    std::process::exit(if ok { 0 } else { 1 });
}
