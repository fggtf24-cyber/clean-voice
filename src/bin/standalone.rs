//! Dev helper: runs the plugin as a standalone app (to look at the GUI without a DAW).
//! cargo run --release --features standalone --bin standalone -- --backend dummy
fn main() {
    nih_plug::nih_export_standalone::<clean_voice::CleanVoice>();
}
