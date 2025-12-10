import("./pkg")
  .then(init)
  .catch((error) =>
    console.error("Failed to load melodeus-browser WASM module", error)
  );

function init(rust) {
  let handle = null;
  let selectedOutput = "";
  let selectedInput = "";
  let monitoringActive = false;
  let pendingMicrophoneRequest = false;
  const permissionState = {
    microphone: "prompt",
  };

  const outputSelect = document.getElementById("output-devices");
  const inputSelect = document.getElementById("input-devices");
  const refreshButton = document.getElementById("refresh-devices");
  const playButton = document.getElementById("play");
  const stopButton = document.getElementById("stop");
  const statusField = document.getElementById("device-status");
  const deviceInfoEl = document.getElementById("device-info");
  const requestPermissionButton = document.getElementById("request-permission");
  const monitorButton = document.getElementById("start-monitor");
  const stopMonitorButton = document.getElementById("stop-monitor");
  const waveformCanvas = document.getElementById("input-waveform");
  const waveformContext = waveformCanvas
    ? waveformCanvas.getContext("2d")
    : null;

  const waveformState = {
    audioContext: null,
    analyser: null,
    animationId: null,
    dataArray: null,
    mediaStream: null,
    source: null,
  };
  const inputDeviceDescriptors = new Map();
  const secureContextForAudio = (() => {
    if (typeof window.isSecureContext === "boolean") {
      if (window.isSecureContext) {
        return true;
      }
    }
    const { protocol, hostname } = window.location;
    if (protocol === "https:") {
      return true;
    }
    return (
      hostname === "localhost" ||
      hostname === "127.0.0.1" ||
      hostname === "[::1]"
    );
  })();

  const setStatus = (message = "") => {
    if (!statusField) {
      return;
    }
    statusField.textContent = message;
  };

  const showError = (prefix, error) => {
    const message = prefix ? `${prefix}: ${String(error)}` : String(error);
    console.error(message);
    setStatus(message);
  };

  const clearStatus = () => setStatus("");

  const reportInsecureContext = () => {
    const message =
      "Microphone input is only available over HTTPS or localhost. Reopen this page securely and try again.";
    console.warn(message);
    setStatus(message);
  };

  const ensureSecureContext = () => {
    if (secureContextForAudio) {
      return true;
    }
    reportInsecureContext();
    return false;
  };

  const updatePermissionButton = () => {
    if (!requestPermissionButton) {
      return;
    }
    if (!secureContextForAudio) {
      requestPermissionButton.value = "Use HTTPS to enable microphone";
      requestPermissionButton.disabled = true;
      return;
    }
    if (pendingMicrophoneRequest) {
      requestPermissionButton.value = "Requesting...";
      requestPermissionButton.disabled = true;
      return;
    }
    const state = permissionState.microphone;
    if (state === "granted") {
      requestPermissionButton.value = "Microphone enabled";
      requestPermissionButton.disabled = true;
    } else if (state === "denied") {
      requestPermissionButton.value = "Retry microphone access";
      requestPermissionButton.disabled = false;
    } else {
      requestPermissionButton.value = "Enable microphone";
      requestPermissionButton.disabled = false;
    }
  };

  const updateMonitorButtons = () => {
    if (monitorButton) {
      monitorButton.disabled =
        monitoringActive || pendingMicrophoneRequest || !secureContextForAudio;
    }
    if (stopMonitorButton) {
      stopMonitorButton.disabled = !monitoringActive;
    }
    updatePermissionButton();
  };

  const setMicrophonePermission = (state) => {
    if (state !== "granted" && state !== "denied" && state !== "prompt") {
      return;
    }
    permissionState.microphone = state;
    updateMonitorButtons();
  };

  const monitorPermissionChanges = () => {
    if (
      !navigator.permissions ||
      typeof navigator.permissions.query !== "function"
    ) {
      updateMonitorButtons();
      return;
    }
    navigator.permissions
      .query({ name: "microphone" })
      .then((status) => {
        setMicrophonePermission(status.state);
        status.onchange = () => setMicrophonePermission(status.state);
      })
      .catch((error) => {
        console.warn("Unable to query microphone permission", error);
        updateMonitorButtons();
      });
  };

  const requestMicrophoneAccess = async () => {
    if (!ensureSecureContext()) {
      return false;
    }
    if (
      !navigator.mediaDevices ||
      typeof navigator.mediaDevices.getUserMedia !== "function"
    ) {
      showError("Browser does not support audio capture", "");
      return false;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.getTracks().forEach((track) => track.stop());
      setMicrophonePermission("granted");
      clearStatus();
      await updateInputDeviceDescriptors();
      return true;
    } catch (error) {
      const name = error && error.name ? error.name : "";
      if (name === "NotAllowedError" || name === "SecurityError") {
        setMicrophonePermission("denied");
        if (!secureContextForAudio) {
          reportInsecureContext();
        } else {
          showError("Microphone access denied", "");
        }
      } else {
        showError("Unable to access microphone", error);
      }
      return false;
    }
  };

  const populateSelect = (selectEl, devices, defaultName, currentSelection) => {
    if (!selectEl) {
      return currentSelection ?? "";
    }

    const list = Array.isArray(devices) ? devices : [];
    const previousValue = currentSelection ?? selectEl.value;

    selectEl.textContent = "";
    const defaultOption = document.createElement("option");
    defaultOption.value = "";
    defaultOption.textContent = defaultName
      ? `System default (${defaultName})`
      : "System default";
    selectEl.appendChild(defaultOption);

    list.forEach((deviceName) => {
      const option = document.createElement("option");
      option.value = deviceName;
      option.textContent = deviceName;
      selectEl.appendChild(option);
    });

    const available = new Set(list);
    if (previousValue && available.has(previousValue)) {
      selectEl.value = previousValue;
      return previousValue;
    }

    if (defaultName && available.has(defaultName)) {
      selectEl.value = defaultName;
      return defaultName;
    }

    selectEl.value = "";
    return "";
  };

  const renderDeviceInfo = (devices) => {
    if (!deviceInfoEl) {
      return;
    }
    const inputs = Array.isArray(devices.inputInfo)
      ? Array.from(devices.inputInfo)
      : [];
    const outputs = Array.isArray(devices.outputInfo)
      ? Array.from(devices.outputInfo)
      : [];

    const formatRate = (v) =>
      typeof v === "number" && Number.isFinite(v) ? `${v} Hz` : "? Hz";
    const formatChannels = (v) =>
      typeof v === "number" && Number.isFinite(v) ? `${v} ch` : "? ch";

    const lines = [];
    lines.push("Input devices:");
    if (inputs.length === 0) {
      lines.push("  (none)");
    } else {
      for (const info of inputs) {
        const name = info.name || info.deviceId || "Unknown input";
        lines.push(
          `  - ${name}: ${formatRate(info.sampleRate)}, ${formatChannels(
            info.channels
          )}`
        );
      }
    }

    lines.push("Output devices:");
    if (outputs.length === 0) {
      lines.push("  (none)");
    } else {
      for (const info of outputs) {
        const name = info.name || "Unknown output";
        lines.push(
          `  - ${name}: ${formatRate(info.sampleRate)}, ${formatChannels(
            info.channels
          )}`
        );
      }
    }

    deviceInfoEl.textContent = lines.join("\n");
  };

  const updateInputDeviceDescriptors = async () => {
    if (
      !navigator.mediaDevices ||
      typeof navigator.mediaDevices.enumerateDevices !== "function"
    ) {
      return;
    }
    try {
      const devices = await navigator.mediaDevices.enumerateDevices();
      inputDeviceDescriptors.clear();
      for (const device of devices) {
        if (device.kind === "audioinput") {
          const label = device.label || device.deviceId;
          inputDeviceDescriptors.set(label, device);
        }
      }
    } catch (error) {
      console.warn("Unable to enumerate browser media devices", error);
    }
  };

  const resolveInputDeviceId = async (deviceName) => {
    if (!deviceName) {
      return null;
    }
    if (!inputDeviceDescriptors.has(deviceName)) {
      await updateInputDeviceDescriptors();
    }
    const descriptor = inputDeviceDescriptors.get(deviceName);
    return descriptor ? descriptor.deviceId : null;
  };

  const refreshDevices = async () => {
    try {
      const devices = await rust.get_audio_devices();
      const outputs = Array.isArray(devices.outputs)
        ? Array.from(devices.outputs)
        : [];
      const inputs = Array.isArray(devices.inputs)
        ? Array.from(devices.inputs)
        : [];

      renderDeviceInfo(devices);

      selectedOutput = populateSelect(
        outputSelect,
        outputs,
        devices.defaultOutput ?? null,
        selectedOutput
      );
      selectedInput = populateSelect(
        inputSelect,
        inputs,
        devices.defaultInput ?? null,
        selectedInput
      );

      if (outputs.length === 0) {
        setStatus("No output devices detected");
      } else if (inputs.length === 0) {
        setStatus("No input devices detected");
      } else {
        clearStatus();
      }
    } catch (error) {
      showError("Unable to enumerate audio devices", error);
    }
    await updateInputDeviceDescriptors();
  };

  const clearWaveform = () => {
    if (waveformCanvas && waveformContext) {
      waveformContext.clearRect(
        0,
        0,
        waveformCanvas.width,
        waveformCanvas.height
      );
    }
  };

  const stopMonitoring = () => {
    if (waveformState.animationId !== null) {
      cancelAnimationFrame(waveformState.animationId);
      waveformState.animationId = null;
    }
    if (waveformState.source) {
      try {
        waveformState.source.disconnect();
      } catch (error) {
        console.warn("Failed to disconnect input source", error);
      }
      waveformState.source = null;
    }
    if (waveformState.analyser) {
      waveformState.analyser.disconnect();
    }
    if (waveformState.mediaStream) {
      waveformState.mediaStream.getTracks().forEach((track) => track.stop());
      waveformState.mediaStream = null;
    }
    if (waveformState.audioContext) {
      waveformState.audioContext.close().catch((error) => {
        console.warn("Failed to close audio context", error);
      });
      waveformState.audioContext = null;
    }
    waveformState.analyser = null;
    waveformState.dataArray = null;
    monitoringActive = false;
    clearWaveform();
    updateMonitorButtons();
  };

  const renderWaveform = () => {
    if (!waveformCanvas || !waveformContext || !waveformState.analyser) {
      waveformState.animationId = null;
      return;
    }

    const { analyser, dataArray } = waveformState;
    analyser.getByteTimeDomainData(dataArray);

    waveformContext.fillStyle = "#202020";
    waveformContext.fillRect(
      0,
      0,
      waveformCanvas.width,
      waveformCanvas.height
    );

    waveformContext.lineWidth = 2;
    waveformContext.strokeStyle = "#4caf50";
    waveformContext.beginPath();

    const sliceWidth = waveformCanvas.width / dataArray.length;
    let x = 0;
    for (let i = 0; i < dataArray.length; i += 1) {
      const value = dataArray[i] / 128.0;
      const y = (value * waveformCanvas.height) / 2;
      if (i === 0) {
        waveformContext.moveTo(x, y);
      } else {
        waveformContext.lineTo(x, y);
      }
      x += sliceWidth;
    }

    waveformContext.lineTo(waveformCanvas.width, waveformCanvas.height / 2);
    waveformContext.stroke();

    waveformState.animationId = window.requestAnimationFrame(renderWaveform);
  };

  const startMonitoring = async () => {
    if (!ensureSecureContext()) {
      return;
    }
    if (
      !navigator.mediaDevices ||
      typeof navigator.mediaDevices.getUserMedia !== "function"
    ) {
      showError("Browser does not support audio capture", "");
      return;
    }

    pendingMicrophoneRequest = true;
    updateMonitorButtons();
    try {
      let targetDeviceId = await resolveInputDeviceId(selectedInput);
      if (
        selectedInput &&
        selectedInput.length > 0 &&
        !targetDeviceId &&
        permissionState.microphone !== "granted"
      ) {
        const granted = await requestMicrophoneAccess();
        if (!granted) {
          return;
        }
        targetDeviceId = await resolveInputDeviceId(selectedInput);
      }
      const audioConstraint =
        targetDeviceId && targetDeviceId.length > 0
          ? { deviceId: { exact: targetDeviceId } }
          : true;
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: audioConstraint,
      });
      setMicrophonePermission("granted");

      stopMonitoring();

      const AudioCtx = window.AudioContext || window.webkitAudioContext;
      if (!AudioCtx) {
        stream.getTracks().forEach((track) => track.stop());
        showError("Web Audio API not supported", "");
        return;
      }

      const audioContext = new AudioCtx();
      if (audioContext.state === "suspended") {
        await audioContext.resume();
      }
      const source = audioContext.createMediaStreamSource(stream);
      const analyser = audioContext.createAnalyser();
      analyser.fftSize = 2048;

      waveformState.audioContext = audioContext;
      waveformState.mediaStream = stream;
      waveformState.source = source;
      waveformState.analyser = analyser;
      waveformState.dataArray = new Uint8Array(analyser.fftSize);

      source.connect(analyser);
      monitoringActive = true;
      renderWaveform();

      await updateInputDeviceDescriptors();
    } catch (error) {
      const name = error && error.name ? error.name : "";
      if (name === "NotAllowedError" || name === "SecurityError") {
        setMicrophonePermission("denied");
        if (!secureContextForAudio) {
          reportInsecureContext();
        } else {
          showError("Microphone access denied", "");
        }
      } else {
        showError("Unable to monitor input device", error);
      }
    } finally {
      pendingMicrophoneRequest = false;
      updateMonitorButtons();
    }
  };

  if (outputSelect) {
    outputSelect.addEventListener("change", () => {
      selectedOutput = outputSelect.value;
    });
  }

  if (inputSelect) {
    inputSelect.addEventListener("change", () => {
      selectedInput = inputSelect.value;
      if (monitoringActive) {
        startMonitoring();
      }
    });
  }

  if (refreshButton) {
    refreshButton.addEventListener("click", async () => {
      clearStatus();
      await refreshDevices();
    });
  }

  if (playButton) {
    playButton.addEventListener("click", () => {
      clearStatus();
      if (handle) {
        handle.free();
        handle = null;
      }
      try {
        const deviceName =
          selectedOutput && selectedOutput.length > 0
            ? selectedOutput
            : null;
        handle = rust.beep_with_output_device(deviceName);
      } catch (error) {
        showError("Failed to start playback", error);
      }
    });
  }

  if (stopButton) {
    stopButton.addEventListener("click", () => {
      if (handle) {
        handle.free();
        handle = null;
      }
    });
  }

  if (requestPermissionButton) {
    requestPermissionButton.addEventListener("click", async () => {
      pendingMicrophoneRequest = true;
      updateMonitorButtons();
      try {
        await requestMicrophoneAccess();
      } finally {
        pendingMicrophoneRequest = false;
        updateMonitorButtons();
      }
    });
  }

  if (monitorButton) {
    monitorButton.addEventListener("click", () => {
      clearStatus();
      startMonitoring();
    });
  }

  if (stopMonitorButton) {
    stopMonitorButton.addEventListener("click", () => {
      stopMonitoring();
    });
  }

  if (!secureContextForAudio) {
    reportInsecureContext();
  }
  monitorPermissionChanges();
  updateMonitorButtons();
  refreshDevices();
}
