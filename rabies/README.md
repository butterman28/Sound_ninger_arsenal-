# Rabies Audio Sampler

A real-time audio chopping sampler and step sequencer built with Rust, `eframe`, and `cpal`.

## 🚀 Quick Start

```bash
cargo run
```

## 🎹 Usage Guide

### 1. Load & Play
1. Click **Load Sample** to import an audio file (WAV, MP3, FLAC, etc.).
2. Press **▶ Play** to start playback.
3. Use the **Waveform Display** to visualize the audio.

### 2. Chopping (The "M" Key)
The best spot for precision chopping is while the track is playing:
1. Press **▶ Play**.
2. Listen for the hit point you want to capture.
3. Press **`M`** on your keyboard to drop a marker at the exact current position.
4. Markers automatically populate the **Sample Pads** below the waveform.

### 3. Pad Layout
Trigger your chops manually using the keyboard or click the on-screen pads.

```text
┌─────┬─────┬─────┬─────┐
│  1  │  2  │  3  │  4  │  ← Pads 1-4
├─────┼─────┼─────┼─────┤
│  Q  │  W  │  E  │  R  │  ← Pads 5-8
├─────┼─────┼─────┼─────┤
│  A  │  S  │  D  │  F  │  ← Pads 9-12
├─────┼─────┼─────┼─────┤
│  Z  │  X  │  C  │  V  │  ← Pads 13-16
└─────┴─────┴─────┴─────┘
```

### 4. Step Sequencer
*   **Grid:** Program patterns for your chops or loaded drum tracks.
*   **ADSR:** Shape each chop/track individually with Attack, Decay, Sustain, and Release knobs.
*   **Piano Roll:** Click **🎹 Piano Roll** for a detailed view of your sequence.
*   **Playback:** Press **▶ Play** in the sequencer header to loop your pattern.

### 5. Navigation
*   **Scroll:** The main interface features a **vertical scrollbar**. If your sequencer tracks or pads exceed the window height, simply scroll down to access them.
*   **Waveform Focus:** Click on a drum track label to view its waveform instead of the main sample.

## 🛠 Features
*   **Real-time Chopping:** Mark points on the fly without stopping playback.
*   **16-Step Sequencer:** Per-step triggering for chops and multi-sample drum tracks.
*   **Per-Voice ADSR:** Individual envelope control for every pad and track.
*   **Waveform Visualization:** Interactive seeking and marker dragging.
*   **Region Playback:** Define custom start/end regions between markers.

---

## FOR,BY & OF THE UNDERDOG