use wasm_bindgen::prelude::*;
use wasm_bindgen::throw_val;
use web_sys::console;
use cpal::{SizedSample, FromSample, SampleFormat};
use js_sys::{Array, Object, Reflect};

// When the `wee_alloc` feature is enabled, this uses `wee_alloc` as the global
// allocator.
//
// If you don't want to use `wee_alloc`, you can safely delete this.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// This is like the `main` function, except for JavaScript.
#[wasm_bindgen(start)]
pub fn main_js() -> Result<(), JsValue> {
    // This provides better error messages in debug mode.
    // It's disabled in release mode so it doesn't bloat up the file size.
    #[cfg(debug_assertions)]
    console_error_panic_hook::set_once();

    Ok(())
}

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;

fn js_error(message: impl Into<String>) -> JsValue {
    JsValue::from_str(&message.into())
}

fn collect_device_names<I>(devices: I, kind: &str) -> Vec<String>
where
    I: IntoIterator<Item = cpal::Device>,
{
    devices
        .into_iter()
        .enumerate()
        .map(|(idx, device)| {
            device.name().unwrap_or_else(|err| {
                console::warn_1(
                    &format!("Failed to read {kind} device name #{idx}: {err}").into(),
                );
                format!("Unnamed {kind} device {}", idx + 1)
            })
        })
        .collect()
}

fn vec_to_js_array(items: &[String]) -> Array {
    let array = Array::new();
    for item in items {
        array.push(&JsValue::from_str(item));
    }
    array
}

fn find_output_device_by_name(
    host: &cpal::Host,
    target_name: &str,
) -> Result<Option<cpal::Device>, JsValue> {
    let devices = host
        .output_devices()
        .map_err(|err| js_error(format!("Failed to enumerate output devices: {err}")))?;

    for device in devices {
        match device.name() {
            Ok(name) if name == target_name => return Ok(Some(device)),
            Ok(_) => continue,
            Err(err) => console::warn_1(
                &format!("Failed to read output device name while matching {target_name}: {err}")
                    .into(),
            ),
        }
    }

    Ok(None)
}

#[wasm_bindgen]
pub struct Handle(Stream);

fn beep_internal(device_name: Option<&str>) -> Result<Handle, JsValue> {
    let host = cpal::default_host();
    let device = if let Some(name) = device_name {
        match find_output_device_by_name(&host, name)? {
            Some(device) => device,
            None => return Err(js_error(format!("Output device '{name}' was not found"))),
        }
    } else {
        host.default_output_device()
            .ok_or_else(|| js_error("Failed to find a default output device"))?
    };
    let config = device.default_output_config().map_err(|err| {
        js_error(format!(
            "Failed to obtain default output configuration for '{}': {err}",
            name_or_unknown(&device)
        ))
    })?;

    let stream = match config.sample_format() {
        SampleFormat::F32 => run::<f32>(&device, &config.into())?,
        SampleFormat::I16 => run::<i16>(&device, &config.into())?,
        SampleFormat::U16 => run::<u16>(&device, &config.into())?,
        // not all supported sample formats are included in this example
        other => {
            return Err(js_error(format!(
                "Unsupported sample format '{other:?}' for selected device"
            )))
        }
    };

    Ok(Handle(stream))
}

fn name_or_unknown(device: &cpal::Device) -> String {
    device
        .name()
        .unwrap_or_else(|_| "Unnamed output device".to_string())
}

#[wasm_bindgen]
pub fn beep() -> Handle {
    match beep_internal(None) {
        Ok(handle) => handle,
        Err(err) => {
            console::error_1(&err);
            throw_val(err);
        }
    }
}

#[wasm_bindgen]
pub fn beep_with_output_device(device_name: Option<String>) -> Result<Handle, JsValue> {
    beep_internal(device_name.as_deref())
}

#[wasm_bindgen]
pub fn get_audio_devices() -> Result<JsValue, JsValue> {
    let host = cpal::default_host();

    let input_devices = host
        .input_devices()
        .map_err(|err| js_error(format!("Failed to enumerate input devices: {err}")))?;
    let output_devices = host
        .output_devices()
        .map_err(|err| js_error(format!("Failed to enumerate output devices: {err}")))?;

    let inputs = collect_device_names(input_devices, "input");
    let outputs = collect_device_names(output_devices, "output");

    let default_input = host.default_input_device().and_then(|device| match device.name() {
        Ok(name) => Some(name),
        Err(err) => {
            console::warn_1(
                &format!("Failed to read default input device name: {err}").into(),
            );
            None
        }
    });

    let default_output = host
        .default_output_device()
        .and_then(|device| match device.name() {
            Ok(name) => Some(name),
            Err(err) => {
                console::warn_1(
                    &format!("Failed to read default output device name: {err}").into(),
                );
                None
            }
        });

    let result = Object::new();
    Reflect::set(
        &result,
        &JsValue::from_str("inputs"),
        &JsValue::from(vec_to_js_array(&inputs)),
    )?;
    Reflect::set(
        &result,
        &JsValue::from_str("outputs"),
        &JsValue::from(vec_to_js_array(&outputs)),
    )?;
    Reflect::set(
        &result,
        &JsValue::from_str("defaultInput"),
        &default_input
            .map(|name| JsValue::from_str(&name))
            .unwrap_or(JsValue::NULL),
    )?;
    Reflect::set(
        &result,
        &JsValue::from_str("defaultOutput"),
        &default_output
            .map(|name| JsValue::from_str(&name))
            .unwrap_or(JsValue::NULL),
    )?;

    Ok(result.into())
}

fn run<T>(device: &cpal::Device, config: &cpal::StreamConfig) -> Result<Stream, JsValue>
where
    T: SizedSample + FromSample<f32>,
{
    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    // Produce a sinusoid of maximum amplitude.
    let mut sample_clock = 0f32;
    let mut next_value = move || {
        sample_clock = (sample_clock + 1.0) % sample_rate;
        (sample_clock * 440.0 * 2.0 * 3.141592 / sample_rate).sin()
    };

    let err_fn = |err| console::error_1(&format!("an error occurred on stream: {}", err).into());

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| write_data(data, channels, &mut next_value),
            err_fn,
            None,
        )
        .map_err(|err| {
            js_error(format!(
                "Failed to build output stream for '{}': {err}",
                name_or_unknown(device)
            ))
        })?;
    stream.play().map_err(|err| {
        js_error(format!(
            "Failed to play output stream for '{}': {err}",
            name_or_unknown(device)
        ))
    })?;
    Ok(stream)
}

fn write_data<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f32)
where
    T: SizedSample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels) {
        let value: T = T::from_sample(next_sample());
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}
