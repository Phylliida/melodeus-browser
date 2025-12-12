import("./pkg").then(({ list_devices, enable_aec }) => {
  const inputSelect = document.getElementById("input-devices");
  const outputSelect = document.getElementById("output-devices");
  const refreshButton = document.getElementById("refresh-devices");
  const enableButton = document.getElementById("enable-aec");
  const statusEl = document.getElementById("status");
  const inputWaveContainer = document.getElementById("input-waves");
  const outputWaveContainer = document.getElementById("output-waves");
  const aecCanvas = document.getElementById("aec-waveform");
  const logEl = document.getElementById("log");

  const log = (...args) => {
    const msg = args
      .map((arg) => {
        if (arg instanceof Error) return arg.stack || arg.message || String(arg);
        if (typeof arg === "object") {
          try {
            return JSON.stringify(arg);
          } catch (_) {
            return String(arg);
          }
        }
        return String(arg);
      })
      .join(" ");
    if (logEl) {
      const line = document.createElement("div");
      line.textContent = msg;
      logEl.appendChild(line);
      logEl.scrollTop = logEl.scrollHeight;
    }
    console.log(msg);
  };

  // Expose a global helper so you can log from the console or other modules.
  window.logMessage = log;

  let devices = { inputs: [], outputs: [] };
  let handle = null;
  let raf = null;

  const setStatus = (msg) => {
    if (statusEl) statusEl.textContent = msg || "";
  };

  const fillSelect = (select, items) => {
    if (!select) return;
    select.textContent = "";
    items.forEach((item, idx) => {
      const opt = document.createElement("option");
      opt.value = item.deviceName;
      opt.textContent = item.deviceName || `device ${idx + 1}`;
      select.appendChild(opt);
    });
  };

  const refreshDevices = async () => {
    try {
      const result = await list_devices();
      devices.inputs = Array.isArray(result.inputs) ? Array.from(result.inputs) : [];
      devices.outputs = Array.isArray(result.outputs) ? Array.from(result.outputs) : [];
      fillSelect(inputSelect, devices.inputs);
      fillSelect(outputSelect, devices.outputs);
      setStatus("");
    } catch (err) {
      console.error(err);
      setStatus("Failed to enumerate devices");
    }
  };

  const ensureCanvas = (container, key, label) => {
    const existing = container.querySelector(`[data-key="${key}"]`);
    if (existing) return existing.querySelector("canvas");
    const wrapper = document.createElement("div");
    wrapper.dataset.key = key;
    const title = document.createElement("div");
    title.textContent = label;
    const canvas = document.createElement("canvas");
    canvas.width = 640;
    canvas.height = 120;
    wrapper.appendChild(title);
    wrapper.appendChild(canvas);
    container.appendChild(wrapper);
    return canvas;
  };

  const drawWaveform = (canvas, data) => {
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx || !data.length) {
      ctx && ctx.clearRect(0, 0, canvas.width, canvas.height);
      return;
    }
    ctx.fillStyle = "#111";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.strokeStyle = "#4caf50";
    ctx.lineWidth = 2;
    ctx.beginPath();
    const step = Math.max(1, Math.floor(data.length / canvas.width));
    const mid = canvas.height / 2;
    for (let x = 0, i = 0; x < canvas.width && i < data.length; x += 1, i += step) {
      const y = mid + data[i] * (mid * 0.9);
      if (x === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  };

  const splitByDevice = (buffer, totalChannels, devicesMeta) => {
    if (!totalChannels || !devicesMeta.length) return [];
    const frames = Math.floor(buffer.length / totalChannels);
    const result = [];
    let offset = 0;
    for (const meta of devicesMeta) {
      const channels = Number(meta.channels) || 1;
      const out = new Float32Array(frames);
      for (let f = 0; f < frames; f += 1) {
        let acc = 0;
        for (let ch = 0; ch < channels; ch += 1) {
          acc += buffer[f * totalChannels + offset + ch] || 0;
        }
        out[f] = acc / channels;
      }
      offset += channels;
      result.push({ name: meta.name || "device", data: out });
    }
    return result;
  };

  const collapseChannels = (buffer, channels) => {
    if (!channels) return new Float32Array();
    const frames = Math.floor(buffer.length / channels);
    const out = new Float32Array(frames);
    for (let f = 0; f < frames; f += 1) {
      let acc = 0;
      for (let ch = 0; ch < channels; ch += 1) {
        acc += buffer[f * channels + ch] || 0;
      }
      out[f] = acc / channels;
    }
    return out;
  };

  const render = (frame) => {
    if (!frame) return;
    const inputs = frame.inputs || new Float32Array();
    const outputs = frame.outputs || new Float32Array();
    const aec = frame.aec || new Float32Array();
    const inputDevices = Array.isArray(frame.inputDevices) ? frame.inputDevices : [];
    const outputDevices = Array.isArray(frame.outputDevices) ? frame.outputDevices : [];
    const inputChannels = Number(frame.inputChannels) || 0;
    const outputChannels = Number(frame.outputChannels) || 0;

    const inputSplits = splitByDevice(inputs, inputChannels, inputDevices);
    const outputSplits = splitByDevice(outputs, outputChannels, outputDevices);

    inputWaveContainer.textContent = "";
    for (const dev of inputSplits) {
      const canvas = ensureCanvas(inputWaveContainer, `in-${dev.name}`, `Input: ${dev.name}`);
      drawWaveform(canvas, dev.data);
    }

    outputWaveContainer.textContent = "";
    for (const dev of outputSplits) {
      const canvas = ensureCanvas(
        outputWaveContainer,
        `out-${dev.name}`,
        `Output: ${dev.name}`
      );
      drawWaveform(canvas, dev.data);
    }

    drawWaveform(aecCanvas, collapseChannels(aec, inputChannels || 1));
  };

  const step = async () => {
    if (!handle) return;
    try {
      const frame = await handle.update();
      render(frame);
      setStatus("");
    } catch (err) {
      console.error(err);
      setStatus("AEC update failed");
      return;
    }
    raf = requestAnimationFrame(step);
  };

  const startAec = async () => {
    if (!enableButton) return;
    enableButton.disabled = true;
    setStatus("Starting AEC...");
    try {
      if (raf) cancelAnimationFrame(raf);
      const inName = inputSelect ? inputSelect.value : null;
      const outName = outputSelect ? outputSelect.value : null;
      handle = await enable_aec(inName || null, outName || null);
      step();
      setStatus("AEC running");
    } catch (err) {
      console.error(err);
      setStatus("Failed to start AEC");
    } finally {
      enableButton.disabled = false;
    }
  };

  refreshButton && refreshButton.addEventListener("click", refreshDevices);
  enableButton && enableButton.addEventListener("click", startAec);
  refreshDevices();
}); 
