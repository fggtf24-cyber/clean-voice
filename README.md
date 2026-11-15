# Clean Voice

VST3/CLAP-плагин шумоподавления голоса на нейросети [DeepFilterNet3](https://github.com/Rikorose/DeepFilterNet).
Убирает клавиатуру, фон, сетевой гул 50/60 Гц и хвост реверберации прямо в монтажке/DAW.
Windows x64. Модель вшита в плагин — никаких внешних файлов и Python.

## Установка

Скачай `CleanVoice-X.Y.Z-setup.exe` со страницы [Releases](https://github.com/fggtf24-cyber/clean-voice/releases) и запусти.
Windows может показать «Неизвестный издатель» → «Подробнее» → «Выполнить в любом случае» (у установщика нет платной подписи).

Плагин ставится в `C:\Program Files\Common Files\VST3\Clean Voice.vst3` и `...\CLAP\Clean Voice.clap`.
Обновления: когда выходит новая версия, в левом нижнем углу окна плагина появляется кнопка **UPDATE TO vX** —
она скачает и запустит установщик.

### Как найти плагин в программах

- **Premiere Pro**: Edit → Preferences → Audio → *Audio Plug-in Manager* → *Scan for Plug-ins*. Эффект: Audio Effects → Clean Voice
  (в Audio Track Mixer он в категории VST3 → Restoration).
- **DaVinci Resolve (Fairlight)**: Preferences → System → Audio Plug-ins → путь `C:\Program Files\Common Files\VST3` → Scan.
  Затем Effects Library → Audio FX → VST.
- **Ableton Live**: Settings → Plug-Ins → включить *Use VST3 Plug-in System Folders* → *Rescan*.
- **OBS**: встроенный фильтр OBS понимает только VST2, VST3 он не видит.

### Стартовые настройки для голоса с клавиатурой

Пресет **Keyboard** (Strength 100 dB, Post-filter вкл.). Если голос звучит «подводно» — уменьши Strength до 20–30 dB.
Гудит сеть → **HUM: 50 Hz** (Европа/СНГ) или 60 Hz (США). Гулкая комната → **Dereverb** 40–60 %, ROOM = примерное время звучания комнаты.
**Delta** — послушать, что именно вырезается (если там слышен голос — убавь Strength).

## Параметры

| Параметр | Что делает |
|---|---|
| Strength | Максимальная глубина подавления, 0–100 dB (100 = без ограничения, 12 = мягко) |
| Mix | Сухой/обработанный сигнал |
| Dereverb / Room | Подавление хвоста реверберации / время реверберации комнаты (T60) |
| Output | Выходная громкость ±12 dB |
| Post-filter / PF AMT | Доп. подавление остатков шума / его сила |
| Delta | Слушать только вырезанное |
| Link (mono) | Стерео обрабатывается как моно (вдвое меньше CPU) |
| Hum | Вырезать сетевой фон 50/60 Гц: сам находит в паузах гармоники, где гул реально есть, и режет только их (до 24-й). На чистой записи ничего не трогает |
| Bypass | Стандартный bypass хоста (задержка сохраняется) |

Задержка 50 мс (при 48 кГц) сообщается хосту — компенсация задержки держит звук в синхроне с видео до сэмпла.
CPU: ~20 % одного ядра на стерео 48 кГц.

## Сборка из исходников

Нужно: [Rust](https://rustup.rs) (`winget install Rustlang.Rustup`),
Visual Studio Build Tools с C++ (`winget install Microsoft.VisualStudio.2022.BuildTools --override "--passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`),
для установщика — [Inno Setup 6](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup`).

```
cargo xtask bundle clean_voice --release     # -> target\bundled\Clean Voice.vst3 / .clap
install.cmd                                  # скопировать в системные папки (закрой DAW)
```

Полезное для разработки:

```
# GUI без DAW
cargo run --release --features standalone --bin standalone -- --backend dummy
# прогнать WAV через DSP-ядро плагина: in out [strength_dB] [hum_Hz] [pf_beta] [dereverb 0..1] [room_s]
cargo run --release --bin process_file -- in.wav out.wav 100 50
# офлайн-тест синхрона/качества против официального deep-filter CLI
cargo run --release --bin offline_test -- <deep-filter.exe> <clean.wav> <noise.wav> <out_dir>
```

Где что: `src/dsp.rs` — ядро (буферизация, ресемплинг, задержка), `src/dereverb.rs` — дереверб,
`src/lib.rs` — параметры и обвязка nih-plug, `src/editor.rs` — интерфейс, `src/update.rs` — проверка обновлений.

### Выпуск новой версии

1. Подними `version` в `Cargo.toml`, закоммить.
2. `powershell -File release.ps1` — соберёт плагин и установщик, создаст тег и GitHub Release.
   Все установленные плагины увидят обновление при следующем открытии окна.

## Лицензия

GPL-3.0 (из-за VST3-биндингов nih-plug). DeepFilterNet — MIT/Apache-2.0.
