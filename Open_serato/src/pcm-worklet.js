class PCMPlayerProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.buffer = null;
    this.f32 = null;
    this.capacity = 0;
    this.channels = 2;
    this.playing = false;

    this.port.onmessage = (e) => {
      const msg = e.data;

      if (msg.type === 'init') {
        this.buffer = msg.sharedBuffer;
        this.f32 = new Float32Array(this.buffer);
        this.capacity = this.f32.length - 2;
      }

      if (msg.type === 'play') {
        this.playing = true;
      }

      if (msg.type === 'pause') {
        this.playing = false;
      }

      if (msg.type === 'stop') {
        this.playing = false;
      }
    };
  }

  process(_, outputs) {
    if (!this.playing || !this.f32) return true;

    const out = outputs[0];
    const ch0 = out[0];
    const ch1 = out[1] || ch0;

    let readPos = Atomics.load(this.f32, 1);
    const writePos = Atomics.load(this.f32, 0);

    for (let i = 0; i < ch0.length; i++) {
      if (readPos === writePos) {
        ch0[i] = 0;
        ch1[i] = 0;
        continue;
      }

      const idx = 2 + readPos;
      ch0[i] = this.f32[idx];
      ch1[i] = this.f32[idx + 1];

      readPos = (readPos + 2) % this.capacity;
    }

    Atomics.store(this.f32, 1, readPos);
    return true;
  }
}

registerProcessor('pcm-player', PCMPlayerProcessor);


