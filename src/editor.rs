use crate::{CvParams, HumMode};
use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, Align2, Color32, FontId, Pos2, Sense, Stroke, Vec2};
use nih_plug_egui::{create_egui_editor, EguiState};
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub fn default_state() -> Arc<EguiState> {
    EguiState::from_size(640, 320)
}


#[derive(Serialize, Deserialize, Clone)]
struct Preset {
    name: String,
    strength: f32,
    mix: f32,
    post_filter: bool,
    pf_beta: f32,
    gain_db: f32,
    link: bool,
    /// None = leave unchanged (factory presets). 0 off, 1 = 50 Hz, 2 = 60 Hz.
    #[serde(default)]
    hum: Option<u8>,
    #[serde(default)]
    dereverb: Option<(f32, f32)>,
}

const HUM_MODES: [HumMode; 3] = [HumMode::Off, HumMode::Hz50, HumMode::Hz60];

/// Global settings shared by all plugin instances: %APPDATA%\CleanVoice\settings.json
#[derive(Serialize, Deserialize)]
struct Store {
    #[serde(default = "yes")]
    english: bool,
    #[serde(default)]
    presets: Vec<Preset>,
}

fn yes() -> bool {
    true
}

impl Default for Store {
    /// English UI by default.
    fn default() -> Self {
        Self { english: true, presets: Vec::new() }
    }
}

impl Store {
    fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join("CleanVoice").join("settings.json"))
    }
    fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    fn save(&self) {
        if let Some(p) = Self::path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap());
        }
    }
}

/// (ru, en, strength dB, post-filter on, post-filter beta, dereverb). Index 0 = plugin defaults.
const FACTORY: &[(&str, &str, f32, bool, f32, f32)] = &[
    ("По умолчанию", "Default", 100.0, false, 0.02, 0.0),
    ("Мягко", "Gentle", 12.0, false, 0.02, 0.0),
    ("Средне", "Medium", 24.0, false, 0.02, 0.0),
    ("Клавиатура", "Keyboard", 100.0, true, 0.02, 0.0),
    ("Максимум", "Maximum", 100.0, true, 0.04, 0.0),
    ("Гулкая комната", "Echoey room", 100.0, false, 0.02, 0.5),
];

fn factory(i: usize) -> Preset {
    let (_, name, strength, post_filter, pf_beta, d) = FACTORY[i];
    Preset { name: name.into(), strength, mix: 1.0, post_filter, pf_beta, gain_db: 0.0, link: false, hum: None, dereverb: Some((d, 0.6)) }
}

struct Ui {
    meter: f32,
    store: Store,
    naming: Option<crate::native::Pending>,
}

fn set<P: Param>(s: &ParamSetter, p: &P, v: P::Plain) {
    s.begin_set_parameter(p);
    s.set_parameter(p, v);
    s.end_set_parameter(p);
}

fn apply(s: &ParamSetter, p: &CvParams, pr: &Preset) {
    set(s, &p.strength, pr.strength);
    set(s, &p.mix, pr.mix);
    set(s, &p.post_filter, pr.post_filter);
    set(s, &p.pf_beta, pr.pf_beta);
    set(s, &p.gain, util::db_to_gain(pr.gain_db));
    set(s, &p.link, pr.link);
    if let Some((amount, room)) = pr.dereverb {
        set(s, &p.dereverb, amount);
        set(s, &p.room, room);
    }
    if let Some(h) = pr.hum {
        set(s, &p.hum, HUM_MODES[(h as usize).min(2)]);
    }
}

fn capture(p: &CvParams, name: String) -> Preset {
    Preset {
        name,
        strength: p.strength.value(),
        mix: p.mix.value(),
        post_filter: p.post_filter.value(),
        pf_beta: p.pf_beta.value(),
        gain_db: util::gain_to_db(p.gain.value()),
        link: p.link.value(),
        hum: HUM_MODES.iter().position(|m| *m == p.hum.value()).map(|i| i as u8),
        dereverb: Some((p.dereverb.value(), p.room.value())),
    }
}

// ---------- Skin B: vintage hardware ----------
const PANEL: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2a);
const FACE: Color32 = Color32::from_rgb(0xe9, 0xe2, 0xcf);
const INK: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2a);
const INK2: Color32 = Color32::from_rgb(0x5f, 0x5e, 0x5a);
const RUST: Color32 = Color32::from_rgb(0x99, 0x3c, 0x1d);
const SKIRT: Color32 = Color32::from_rgb(0xd3, 0xd1, 0xc7);
const LED_ON: Color32 = Color32::from_rgb(0xe2, 0x4b, 0x4a);
const LED_OFF: Color32 = Color32::from_rgb(0x88, 0x87, 0x80);
const VU_FACE: Color32 = Color32::from_rgb(0xf5, 0xe9, 0xc4);
const AMBER: Color32 = Color32::from_rgb(0xef, 0x9f, 0x27);

fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Hardware-style knob at `c`: drag up/down (Shift = fine), double-click = default.
fn knob(ui: &mut egui::Ui, s: &ParamSetter, p: &FloatParam, c: Pos2, r: f32, label: &str, hint: &str) {
    let rect = egui::Rect::from_center_size(c, Vec2::splat(r * 2.4));
    let resp = ui.interact(rect, ui.id().with(p.name()), Sense::click_and_drag());
    if resp.drag_started() {
        s.begin_set_parameter(p);
    }
    if resp.dragged() {
        let speed = if ui.input(|i| i.modifiers.shift) { 0.0008 } else { 0.005 };
        let n = (p.unmodulated_normalized_value() - resp.drag_delta().y * speed).clamp(0.0, 1.0);
        s.set_parameter_normalized(p, n);
    }
    if resp.drag_stopped() {
        s.end_set_parameter(p);
    }
    if resp.double_clicked() {
        set(s, p, p.default_plain_value());
    }
    let pt = ui.painter();
    // Scale ticks around the knob (270 degree sweep, gap at the bottom).
    let (a0, a1) = (0.75 * PI, 2.25 * PI);
    for i in 0..=10 {
        let a = a0 + (a1 - a0) * i as f32 / 10.0;
        let d = Vec2::new(a.cos(), a.sin());
        let len = if i % 5 == 0 { 9.0 } else { 6.0 };
        pt.line_segment([c + d * (r + 3.0), c + d * (r + len)], Stroke::new(1.2_f32, INK2));
    }
    pt.circle_filled(c, r, SKIRT);
    pt.circle_filled(c, r * 0.76, INK);
    let a = a0 + (a1 - a0) * p.unmodulated_normalized_value();
    let d = Vec2::new(a.cos(), a.sin());
    pt.line_segment([c + d * (r * 0.15), c + d * (r * 0.7)], Stroke::new(3.0_f32, Color32::from_rgb(0xf1, 0xef, 0xe8)));
    pt.text(c + Vec2::new(0.0, r + 20.0), Align2::CENTER_CENTER, label, font(11.0), INK);
    pt.text(c + Vec2::new(0.0, r + 34.0), Align2::CENTER_CENTER, p.to_string(), font(11.0), INK2);
    resp.on_hover_text(hint);
}

/// LED + label, the whole row is clickable.
fn led(ui: &mut egui::Ui, at: Pos2, on: bool, label: &str) -> bool {
    let rect = egui::Rect::from_min_size(at - Vec2::new(8.0, 9.0), Vec2::new(105.0, 18.0));
    let resp = ui.interact(rect, ui.id().with(("led", label)), Sense::click());
    let pt = ui.painter();
    pt.circle_filled(at, 6.5, if on { LED_ON } else { LED_OFF });
    pt.circle_stroke(at, 6.5, Stroke::new(1.0_f32, INK2));
    if on {
        pt.circle_filled(at + Vec2::new(-2.0, -2.0), 1.8, Color32::from_rgb(0xf7, 0xc1, 0xc1));
    }
    pt.text(at + Vec2::new(13.0, 0.0), Align2::LEFT_CENTER, label, font(11.0), INK);
    resp.clicked()
}

/// Analog gain-reduction needle meter (0..40 dB, sqrt scale like a VU).
fn vu(ui: &egui::Ui, r: egui::Rect, gr_db: f32, caption: &str) {
    let pt = ui.painter();
    pt.rect_filled(r, 4.0, PANEL);
    let face = r.shrink(7.0);
    pt.rect_filled(face, 3.0, VU_FACE);
    let pivot = Pos2::new(face.center().x, face.bottom() + 18.0);
    let rad = face.height() * 0.95;
    let ang = |db: f32| {
        let x = (db.clamp(0.0, 40.0) / 40.0).sqrt();
        (-140.0_f32 + 100.0 * x).to_radians()
    };
    let at = |db: f32, k: f32| pivot + Vec2::new(ang(db).cos(), ang(db).sin()) * rad * k;
    let arc = |from: f32, to: f32, w: f32, col: Color32| {
        let pts: Vec<Pos2> = (0..=30).map(|i| at(from + (to - from) * i as f32 / 30.0, 0.78)).collect();
        pt.add(egui::Shape::line(pts, Stroke::new(w, col)));
    };
    arc(0.0, 40.0, 1.0, INK2);
    arc(20.0, 40.0, 3.5, Color32::from_rgb(0xa3, 0x2d, 0x2d));
    for (db, txt) in [(0.0, "0"), (3.0, "3"), (6.0, "6"), (10.0, "10"), (20.0, "20"), (40.0, "40")] {
        pt.line_segment([at(db, 0.78), at(db, 0.86)], Stroke::new(1.2_f32, INK));
        pt.text(at(db, 0.95), Align2::CENTER_CENTER, txt, font(10.0), INK);
    }
    pt.text(Pos2::new(face.right() - 6.0, face.bottom() - 24.0), Align2::RIGHT_CENTER, caption, font(10.0), INK2);
    let tip = at(gr_db, 0.88);
    let base = pivot + (tip - pivot) * 0.38;
    pt.line_segment([base, tip], Stroke::new(2.0_f32, INK));
    pt.text(Pos2::new(face.right() - 6.0, face.bottom() - 10.0), Align2::RIGHT_CENTER, if gr_db < 0.05 { "0.0".to_string() } else { format!("-{:.1}", gr_db) }, font(10.0), RUST);
}

fn screw(pt: &egui::Painter, c: Pos2) {
    pt.circle_filled(c, 4.5, Color32::from_rgb(0xb4, 0xb2, 0xa9));
    pt.line_segment([c + Vec2::new(-3.0, -1.5), c + Vec2::new(3.0, 1.5)], Stroke::new(1.0_f32, INK2));
}

/// Small flat button drawn on the faceplate.
fn plate_button(ui: &mut egui::Ui, r: egui::Rect, text: &str) -> bool {
    let resp = ui.interact(r, ui.id().with(("btn", text)), Sense::click());
    let fill = if resp.hovered() { SKIRT } else { Color32::from_rgb(0xdd, 0xd5, 0xbd) };
    ui.painter().rect_filled(r, 3.0, fill);
    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0_f32, INK2), egui::StrokeKind::Inside);
    ui.painter().text(r.center(), Align2::CENTER_CENTER, text, font(11.0), INK);
    resp.clicked()
}

pub fn create(params: Arc<CvParams>, reduction: Arc<AtomicF32>) -> Option<Box<dyn Editor>> {
    let state = Ui { meter: 0.0, store: Store::load(), naming: None };
    create_egui_editor(params.editor_state.clone(), state, |_, _| {}, move |ctx, setter, st| {
        let en = st.store.english;
        let t = |ru: &'static str, eng: &'static str| if en { eng } else { ru };
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(PANEL)).show(ctx, |ui| {
            let o = ui.max_rect().min.to_vec2();
            let p = |x: f32, y: f32| Pos2::new(x, y) + o;
            let rr = |x0: f32, y0: f32, x1: f32, y1: f32| egui::Rect::from_min_max(p(x0, y0), p(x1, y1));
            let pt = ui.painter().clone();
            pt.rect_filled(rr(8.0, 8.0, 632.0, 312.0), 6.0, FACE);
            for (x, y) in [(20.0, 20.0), (620.0, 20.0), (20.0, 300.0), (620.0, 300.0)] {
                screw(&pt, p(x, y));
            }
            pt.text(p(36.0, 38.0), Align2::LEFT_CENTER, "Clean Voice", font(22.0), INK);
            pt.text(p(37.0, 60.0), Align2::LEFT_CENTER, "NEURAL VOICE RESTORER  ·  MODEL DF-3", font(10.0), RUST);

            // Preset "LCD" + save/delete.
            let mut current = params.preset.read().unwrap().clone();
            let factory_idx = current.strip_prefix('#').and_then(|n| n.parse::<usize>().ok()).filter(|&i| i < FACTORY.len());
            let lcd = rr(36.0, 76.0, 236.0, 98.0);
            pt.rect_filled(lcd, 3.0, PANEL);
            // Result of the native "preset name" dialog, if one is open.
            let finished = st.naming.as_ref().and_then(|n| n.lock().unwrap().take());
            if let Some(res) = finished {
                st.naming = None;
                if let Some(name) = res {
                    st.store.presets.retain(|p| p.name != name);
                    st.store.presets.push(capture(&params, name.clone()));
                    st.store.save();
                    current = name;
                }
            }
            if st.naming.is_some() {
                pt.text(lcd.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, t("ВВЕДИТЕ ИМЯ В ОКНЕ…", "ENTER NAME IN DIALOG…"), font(12.0), AMBER);
            } else {
                let shown = match factory_idx {
                    Some(i) => t(FACTORY[i].0, FACTORY[i].1).to_string(),
                    None if current.is_empty() => "-".to_string(),
                    None => current.clone(),
                };
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(lcd.shrink2(Vec2::new(4.0, 1.0))));
                let v = child.visuals_mut();
                v.override_text_color = Some(AMBER);
                for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
                    w.weak_bg_fill = PANEL;
                    w.bg_stroke = Stroke::NONE;
                }
                egui::ComboBox::from_id_salt("preset").selected_text(shown.to_uppercase()).width(192.0).show_ui(&mut child, |ui| {
                    for (i, &(ru, eng, ..)) in FACTORY.iter().enumerate() {
                        if ui.selectable_label(factory_idx == Some(i), t(ru, eng)).clicked() {
                            apply(setter, &params, &factory(i));
                            current = format!("#{i}");
                        }
                    }
                    if !st.store.presets.is_empty() {
                        ui.separator();
                    }
                    for pr in &st.store.presets {
                        if ui.selectable_label(current == pr.name, &pr.name).clicked() {
                            apply(setter, &params, pr);
                            current = pr.name.clone();
                        }
                    }
                });
                if plate_button(ui, rr(242.0, 76.0, 300.0, 98.0), t("СОХР.", "SAVE")) {
                    st.naming = Some(crate::native::ask_text(t("Название пресета:", "Preset name:")));
                }
                if let Some(i) = st.store.presets.iter().position(|p| p.name == current) {
                    if plate_button(ui, rr(306.0, 76.0, 364.0, 98.0), t("УДАЛ.", "DELETE")) {
                        st.store.presets.remove(i);
                        st.store.save();
                        current = "#0".into();
                    }
                }
            }
            if *params.preset.read().unwrap() != current {
                *params.preset.write().unwrap() = current;
            }

            // Needle meter with simple VU ballistics.
            let target = reduction.load(Ordering::Relaxed);
            st.meter += (target - st.meter) * 0.18;
            vu(ui, rr(400.0, 22.0, 612.0, 138.0), st.meter, t("ПОДАВЛЕНИЕ, dB", "dB GAIN REDUCTION"));

            // Big knobs.
            let hint = t("Тянуть вверх/вниз, Shift - точно, двойной клик - сброс", "Drag up/down, Shift = fine, double-click = reset");
            knob(ui, setter, &params.strength, p(80.0, 172.0), 34.0, t("СИЛА", "STRENGTH"), hint);
            knob(ui, setter, &params.mix, p(180.0, 172.0), 27.0, t("МИКС", "MIX"), hint);
            knob(ui, setter, &params.dereverb, p(266.0, 172.0), 27.0, t("ДЕРЕВЕРБ", "DEREVERB"), hint);
            knob(ui, setter, &params.gain, p(352.0, 172.0), 27.0, t("ВЫХОД", "OUTPUT"), hint);

            // Switches.
            if led(ui, p(420.0, 162.0), params.post_filter.value(), t("ПОСТ-ФИЛЬТР", "POST-FILTER")) {
                set(setter, &params.post_filter, !params.post_filter.value());
            }
            if led(ui, p(420.0, 186.0), params.delta.value(), t("ДЕЛЬТА", "DELTA")) {
                set(setter, &params.delta, !params.delta.value());
            }
            if led(ui, p(528.0, 162.0), params.link.value(), t("МОНО (LINK)", "LINK (MONO)")) {
                set(setter, &params.link, !params.link.value());
            }
            let hum = params.hum.value();
            let hum_label = match hum {
                HumMode::Off => t("ФОН: ВЫКЛ", "HUM: OFF"),
                HumMode::Hz50 => t("ФОН: 50 Гц", "HUM: 50 Hz"),
                HumMode::Hz60 => t("ФОН: 60 Гц", "HUM: 60 Hz"),
            };
            if led(ui, p(528.0, 186.0), hum != HumMode::Off, hum_label) {
                let next = HUM_MODES[(HUM_MODES.iter().position(|m| *m == hum).unwrap() + 1) % 3];
                set(setter, &params.hum, next);
            }

            // Trim knobs.
            knob(ui, setter, &params.pf_beta, p(440.0, 246.0), 15.0, t("ПОСТ-Ф.", "PF AMT"), hint);
            knob(ui, setter, &params.room, p(520.0, 246.0), 15.0, t("КОМНАТА", "ROOM"), hint);

            // Version + updates (bottom-left).
            let upd = crate::update::state();
            let us = upd.lock().unwrap().clone();
            let ver = concat!("v", env!("CARGO_PKG_VERSION"));
            match us {
                crate::update::State::Available { version, .. } => {
                    let label = format!("{} v{version}", t("ОБНОВИТЬ ДО", "UPDATE TO"));
                    if plate_button(ui, rr(36.0, 284.0, 200.0, 302.0), &label) {
                        crate::update::install(upd.clone());
                    }
                }
                crate::update::State::Downloading => {
                    pt.text(p(36.0, 293.0), Align2::LEFT_CENTER, t("ЗАГРУЗКА ОБНОВЛЕНИЯ…", "DOWNLOADING UPDATE…"), font(10.0), RUST);
                }
                crate::update::State::Launched => {
                    pt.text(p(36.0, 293.0), Align2::LEFT_CENTER, t("УСТАНОВЩИК ЗАПУЩЕН", "INSTALLER STARTED"), font(10.0), RUST);
                }
                _ => {
                    pt.text(p(36.0, 293.0), Align2::LEFT_CENTER, ver, font(10.0), INK2);
                }
            }

            if plate_button(ui, rr(578.0, 284.0, 612.0, 302.0), if en { "RU" } else { "EN" }) {
                st.store.english = !en;
                st.store.save();
            }
        });
        ctx.request_repaint();
    })
}
