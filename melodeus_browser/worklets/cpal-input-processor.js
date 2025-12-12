class CpalInputProcessor extends AudioWorkletProcessor {
  process(inputs) {
    const input = inputs[0]; // array per channel
    this.port.postMessage(input);
    return true;
  }
}

registerProcessor("cpal-input-processor", CpalInputProcessor);
