#![allow(unsafe_op_in_unsafe_fn)]

mod cpal_webaudio_inputs;
mod aec;
#[path = "speex/lib.rs"]
pub mod speex;

use aec::{AecConfig, AecStream, InputDeviceConfig, OutputDeviceConfig, OutputStreamAlignerProducer};
use js_sys::{Array, Float32Array, Object, Reflect};
use wasm_bindgen::prelude::*;

const HISTORY_LEN: usize = 120;
const CALIBRATION_PACKETS: u32 = 15;
const AUDIO_BUFFER_SECONDS: u32 = 5;
const RESAMPLER_QUALITY: i32 = 5;
const OUTPUT_FRAME_SIZE: u32 = 480; // ~10ms at 48 kHz, small for low latency

const TARGET_SAMPLE_RATE: u32 = 16_000;
const FRAME_SIZE_MS: usize = 10;
const FILTER_LENGTH_MS: usize = 100;

#[wasm_bindgen(start)]
pub fn main_js() {
    // Always install the panic hook so wasm panics show up in the browser console
    console_error_panic_hook::set_once();
}

fn aec_config() -> AecConfig {
    let frame_size = TARGET_SAMPLE_RATE as usize * FRAME_SIZE_MS / 1000;
    let filter_len = TARGET_SAMPLE_RATE as usize * FILTER_LENGTH_MS / 1000;
    AecConfig::new(TARGET_SAMPLE_RATE, frame_size, filter_len)
}

fn js_err(err: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&err.to_string())
}

fn inputs_to_js(configs: &[InputDeviceConfig]) -> Result<Array, JsValue> {
    let array = Array::new();
    for cfg in configs {
        let obj = Object::new();
        Reflect::set(&obj, &"hostId".into(), &cfg.host_id.name().into())?;
        Reflect::set(&obj, &"deviceName".into(), &cfg.device_name.clone().into())?;
        Reflect::set(&obj, &"channels".into(), &(cfg.channels as f64).into())?;
        Reflect::set(&obj, &"sampleRate".into(), &(cfg.sample_rate as f64).into())?;
        Reflect::set(&obj, &"sampleFormat".into(), &format!("{:?}", cfg.sample_format).into())?;
        array.push(&obj);
    }
    Ok(array)
}

fn outputs_to_js(configs: &[OutputDeviceConfig]) -> Result<Array, JsValue> {
    let array = Array::new();
    for cfg in configs {
        let obj = Object::new();
        Reflect::set(&obj, &"hostId".into(), &cfg.host_id.name().into())?;
        Reflect::set(&obj, &"deviceName".into(), &cfg.device_name.clone().into())?;
        Reflect::set(&obj, &"channels".into(), &(cfg.channels as f64).into())?;
        Reflect::set(&obj, &"sampleRate".into(), &(cfg.sample_rate as f64).into())?;
        Reflect::set(&obj, &"sampleFormat".into(), &format!("{:?}", cfg.sample_format).into())?;
        array.push(&obj);
    }
    Ok(array)
}

fn pick_input_config<'a>(
    configs: &'a [InputDeviceConfig],
    target_device: Option<&str>,
) -> Option<&'a InputDeviceConfig> {
    if let Some(name) = target_device {
        configs.iter().find(|cfg| cfg.device_name == name)
    } else {
        configs.first()
    }
}

fn pick_output_config<'a>(
    configs: &'a [OutputDeviceConfig],
    target_device: Option<&str>,
) -> Option<&'a OutputDeviceConfig> {
    if let Some(name) = target_device {
        configs.iter().find(|cfg| cfg.device_name == name)
    } else {
        configs.first()
    }
}

fn normalize_i16(slice: &[i16]) -> Vec<f32> {
    if slice.is_empty() {
        return Vec::new();
    }
    let scale = i16::MAX as f32;
    slice.iter().map(|s| *s as f32 / scale).collect()
}

#[wasm_bindgen]
pub async fn list_devices() -> Result<JsValue, JsValue> {
    let inputs = aec::get_supported_input_configs(
        HISTORY_LEN,
        CALIBRATION_PACKETS,
        AUDIO_BUFFER_SECONDS,
        RESAMPLER_QUALITY,
    )
    .await
    .map_err(js_err)?;

    let inputs = aec::get_supported_input_configs(
        HISTORY_LEN,
        CALIBRATION_PACKETS,
        AUDIO_BUFFER_SECONDS,
        RESAMPLER_QUALITY,
    )
    .await
    .map_err(js_err)?;

    let inputs = aec::get_supported_input_configs(
        HISTORY_LEN,
        CALIBRATION_PACKETS,
        AUDIO_BUFFER_SECONDS,
        RESAMPLER_QUALITY,
    )
    .await
    .map_err(js_err)?;

    let inputs = aec::get_supported_input_configs(
        HISTORY_LEN,
        CALIBRATION_PACKETS,
        AUDIO_BUFFER_SECONDS,
        RESAMPLER_QUALITY,
    )
    .await
    .map_err(js_err)?;

    let outputs = aec::get_supported_output_configs(
        HISTORY_LEN,
        CALIBRATION_PACKETS,
        AUDIO_BUFFER_SECONDS,
        RESAMPLER_QUALITY,
        OUTPUT_FRAME_SIZE,
    )
    .await
    .map_err(js_err)?;

    // default config is first entry per device
    let inputs_flat: Vec<InputDeviceConfig> =
        inputs.into_iter().filter_map(|group| group.into_iter().next()).collect();
    let outputs_flat: Vec<OutputDeviceConfig> =
        outputs.into_iter().filter_map(|group| group.into_iter().next()).collect();

    let inputs_js: JsValue = inputs_to_js(&inputs_flat)?.into();
    let outputs_js: JsValue = outputs_to_js(&outputs_flat)?.into();
    let result = Object::new();
    Reflect::set(&result, &"inputs".into(), &inputs_js)?;
    Reflect::set(&result, &"outputs".into(), &outputs_js)?;
    Ok(result.into())
}

#[wasm_bindgen]
pub struct AecHandle {
    stream: AecStream,
    output_producers: Vec<OutputStreamAlignerProducer>,
    inputs: Vec<InputDeviceConfig>,
    outputs: Vec<OutputDeviceConfig>,
}

#[wasm_bindgen]
pub async fn enable_aec(
    input_device: Option<String>,
    output_device: Option<String>,
) -> Result<AecHandle, JsValue> {
    let inputs = aec::get_supported_input_configs(
        HISTORY_LEN,
        CALIBRATION_PACKETS,
        AUDIO_BUFFER_SECONDS,
        RESAMPLER_QUALITY,
    )
    .await
    .map_err(js_err)?;
    let outputs = aec::get_supported_output_configs(
        HISTORY_LEN,
        CALIBRATION_PACKETS,
        AUDIO_BUFFER_SECONDS,
        RESAMPLER_QUALITY,
        OUTPUT_FRAME_SIZE,
    )
    .await
    .map_err(js_err)?;

    let inputs_flat: Vec<InputDeviceConfig> =
        inputs.into_iter().filter_map(|group| group.into_iter().next()).collect();
    let outputs_flat: Vec<OutputDeviceConfig> =
        outputs.into_iter().filter_map(|group| group.into_iter().next()).collect();

    let input_cfg = pick_input_config(&inputs_flat, input_device.as_deref())
        .ok_or_else(|| js_err("no input device available"))?
        .clone();
    let output_cfg = pick_output_config(&outputs_flat, output_device.as_deref())
        .ok_or_else(|| js_err("no output device available"))?
        .clone();

    let mut stream = AecStream::new(aec_config()).map_err(js_err)?;

    let mut output_producers = Vec::new();
    let producer = stream.add_output_device(&output_cfg).await.map_err(js_err)?;
    output_producers.push(producer);

    stream.add_input_device(&input_cfg).await.map_err(js_err)?;
    stream
        .calibrate(output_producers.as_mut_slice(), false).await
        .map_err(js_err)?;

    Ok(AecHandle {
        stream,
        output_producers,
        inputs: vec![input_cfg],
        outputs: vec![output_cfg],
    })
}

#[wasm_bindgen]
impl AecHandle {
    pub async fn update(&mut self) -> Result<JsValue, JsValue> {
        let input_channels = self.stream.num_input_channels();
        let output_channels = self.stream.num_output_channels();
        let (inputs_i16, outputs_i16, aec_out, start_micros, end_micros) =
            self.stream.update_debug().await.map_err(js_err)?;

        let inputs = normalize_i16(inputs_i16);
        let outputs = normalize_i16(outputs_i16);
        let aec = aec_out.to_vec();

        let obj = Object::new();
        Reflect::set(&obj, &"inputs".into(), &Float32Array::from(inputs.as_slice()))?;
        Reflect::set(&obj, &"outputs".into(), &Float32Array::from(outputs.as_slice()))?;
        Reflect::set(&obj, &"aec".into(), &Float32Array::from(aec.as_slice()))?;
        Reflect::set(
            &obj,
            &"inputChannels".into(),
            &(input_channels as f64).into(),
        )?;
        Reflect::set(
            &obj,
            &"outputChannels".into(),
            &(output_channels as f64).into(),
        )?;

        let inputs_meta = Array::new();
        for cfg in &self.inputs {
            let o = Object::new();
            Reflect::set(&o, &"name".into(), &cfg.device_name.clone().into())?;
            Reflect::set(&o, &"channels".into(), &(cfg.channels as f64).into())?;
            inputs_meta.push(&o);
        }
        let outputs_meta = Array::new();
        for cfg in &self.outputs {
            let o = Object::new();
            Reflect::set(&o, &"name".into(), &cfg.device_name.clone().into())?;
            Reflect::set(&o, &"channels".into(), &(cfg.channels as f64).into())?;
            outputs_meta.push(&o);
        }
        Reflect::set(&obj, &"inputDevices".into(), &inputs_meta)?;
        Reflect::set(&obj, &"outputDevices".into(), &outputs_meta)?;
        Reflect::set(
            &obj,
            &"startMicros".into(),
            &(start_micros as f64).into(),
        )?;
        Reflect::set(&obj, &"endMicros".into(), &(end_micros as f64).into())?;

        Ok(obj.into())
    }
}
