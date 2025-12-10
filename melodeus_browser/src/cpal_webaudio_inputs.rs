
extern crate js_sys;
extern crate wasm_bindgen;
extern crate web_sys;

use js_sys::Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, MediaDevices, MediaStream, MediaStreamConstraints, MediaStreamTrack, Navigator,
};


use wasm_bindgen_futures::JsFuture;
use self::web_sys::{AudioContext, AudioContextOptions, MediaStream, MediaStreamConstraints, MediaDevices, Navigator, BlobPropertyBag, Url};
use js_sys::{Float32Array};
use std::ops::DerefMut;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;


/// Discovered details for a specific audio input device obtained via `getUserMedia` constraints.
#[derive(Clone, Debug)]
pub struct InputDeviceInfo {
    pub device_id: String,
    pub label: Option<String>,
    pub sample_rate: u32,
    pub channels: usize,
}
fn buffer_time_step_secs(buffer_size_frames: usize, sample_rate: u32) -> f64 {
    buffer_size_frames as f64 / (sample_rate as f64)
}

pub async fn request_input_access() -> Result<(MediaDevices, MediaStream), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window not available"))?;
    let navigator: Navigator = window.navigator();
    let media_devices: MediaDevices = navigator.media_devices()?;

    let constraints = MediaStreamConstraints::new();
    constraints.set_audio(&JsValue::from_bool(true));
    constraints.set_video(&JsValue::from_bool(false));

    let default_stream = media_devices.get_user_media_with_constraints(&constraints)?;
    let default_stream = JsFuture::from(default_stream).await?;
    let default_stream: MediaStream = default_stream.dyn_into()?;
    Ok((media_devices, default_stream))
}

pub async fn get_input_devices() -> Result<Vec<InputDeviceInfo>, JsValue> {
    let (media_devices, default_stream) = request_input_access().await?;

    // Now enumerate concrete audio input devices and probe each with its deviceId constraint.
    let devices = JsFuture::from(media_devices.enumerate_devices()?).await?;
    let devices: js_sys::Array = devices.dyn_into()?;
    let mut infos = Vec::new();

    for device in devices.iter() {
        let kind = js_sys::Reflect::get(&device, &JsValue::from_str("kind"))
            .ok()
            .and_then(|k| k.as_string());
        if kind.as_deref() != Some("audioinput") {
            continue;
        }

        let device_id = js_sys::Reflect::get(&device, &JsValue::from_str("deviceId"))
            .ok()
            .and_then(|id| id.as_string())
            .unwrap_or_default();
        if device_id.is_empty() {
            continue;
        }

        let label = js_sys::Reflect::get(&device, &JsValue::from_str("label"))
            .ok()
            .and_then(|l| l.as_string())
            .filter(|l| !l.is_empty());

        // Probe the specific device so we can grab its channel count and sample rate.
        let mut constraints = MediaStreamConstraints::new();
        constraints.set_video(&JsValue::from_bool(false));

        let audio_obj = js_sys::Object::new();
        let device_obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &device_obj,
            &JsValue::from_str("exact"),
            &JsValue::from_str(&device_id),
        );
        let _ = js_sys::Reflect::set(&audio_obj, &JsValue::from_str("deviceId"), &device_obj);
        constraints.set_audio(&audio_obj.into());

        let device_stream = media_devices.get_user_media_with_constraints(&constraints)?;
        let device_stream = JsFuture::from(device_stream).await?;
        let device_stream: MediaStream = device_stream.dyn_into()?;

        let test_context = AudioContext::new()?;
        // Necessary to read sample rate in some browsers.
        let source = test_context.create_media_stream_source(&device_stream)?;

        let sample_rate = test_context.sample_rate() as u32;
        let channels = source.channel_count() as usize;

        // Stop tracks to release the device after probing.
        for track in device_stream.get_tracks().iter() {
            if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
                track.stop();
            }
        }

        infos.push(InputDeviceInfo {
            device_id,
            label,
            sample_rate,
            channels,
        });
    }

    // Release the default stream we opened to get permission.
    for track in default_stream.get_tracks().iter() {
        if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
            track.stop();
        }
    }

    Ok(infos)
}


pub async fn build_input_stream<D, E>(
    device_info: InputDeviceInfo,
    data_callback: D,
    error_callback: E
) -> Result<web_sys::AudioContext, JsValue>
    where
        D: FnMut(&[f32]) + Send + 'static,
        E: FnMut(JsValue) + Send + 'static,
{
    Ok(build_input_stream_raw(device_info, data_callback).await.map_err(
        |err| -> JsValue {
            (error_callback)(err.clone());
            err
        }
    )?)
}

pub async fn build_input_stream_raw<D, E>(
    device_info: InputDeviceInfo,
    mut data_callback: D,
) -> Result<web_sys::AudioContext, JsValue>
    where
        D: FnMut(&[f32]) + Send + 'static,
        E: FnMut(JsValue) + Send + 'static,
{

    let ctx = web_sys::AudioContext::new()?;
    // SAFETY: WASM is single-threaded, so Arc is safe even though AudioContext is not Send/Sync
    #[allow(clippy::arc_with_non_send_sync)]
    let window = web_sys::window()
                        .ok_or_else(|| JsValue::from_str("window not available"))?;
    let navigator: Navigator = window.navigator();
    let media_devices: MediaDevices = navigator.media_devices()?;

    let constraints = MediaStreamConstraints::new();
    constraints.set_audio(&JsValue::from_bool(true));

    let constraints = MediaStreamConstraints::new();
    constraints.set_video(&JsValue::from_bool(false));

    // this allows us to specify a device:
    // audio: {
    //   deviceId: {
    //     exact: deviceId,
    //   },
    // },
    let audio_obj = js_sys::Object::new();
    let device_obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&device_obj, &JsValue::from_str("exact"), &JsValue::from_str(&device_info.device_id));
    let _ = js_sys::Reflect::set(&audio_obj, &JsValue::from_str("deviceId"), &device_obj);
    constraints.set_audio(&audio_obj.into());

    let stream = media_devices.get_user_media_with_constraints(&constraints)?;
    let stream = JsFuture::from(stream).await?;
    let stream: MediaStream = stream.dyn_into()?;
    
    let source = ctx.create_media_stream_source(&stream)?;

    // must be fetched after call to create_media_stream_source (before that, it will not be populated)
    let sample_rate = ctx.sample_rate() as u32;

    let processor_js_code = r#"
        class CpalInputProcessor extends AudioWorkletProcessor {
            process(inputs, outputs, parameters) {
                const input = inputs[0]; // only one input device as input, just grab it
                // it is an array of samples, one array for each channel
                this.port.postMessage( input );
                return true;
            }
        }

        registerProcessor('cpal-input-processor', CpalInputProcessor);
    "#;

    let blob_parts = js_sys::Array::new();
    blob_parts.push(&wasm_bindgen::JsValue::from_str(processor_js_code));

    let type_: BlobPropertyBag = BlobPropertyBag::new();
    type_.set_type("application/javascript");

    let blob = web_sys::Blob::new_with_str_sequence_and_options(&blob_parts, &type_).unwrap();

    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();

    let processor = ctx
        .audio_worklet()
        .expect("Failed to get audio worklet")
        .add_module(&url)
        .unwrap();

    JsFuture::from(processor).await.unwrap();

    web_sys::Url::revoke_object_url(&url).unwrap();

    let worklet_node = web_sys::AudioWorkletNode::new(ctx.as_ref(), "cpal-input-processor")
        .expect("Failed to create audio worklet node");

    source.connect_with_audio_node(&worklet_node).unwrap();

    let mut output_buf: Vec<f32> = Vec::new();

    // Float32Array
    let js_closure = Closure::wrap(Box::new(move |msg: wasm_bindgen::JsValue| {
        
        let msg_event = msg.dyn_into::<web_sys::MessageEvent>().unwrap();

        let data = msg_event.data();

        let data : Vec<Vec<f32>> = Array::from(&data).iter()
                    .map(|v| Float32Array::from(v).to_vec())
                    .collect();

        let channels = data.len();

        if channels == 0 {
            return;
        }
        
        let frames = data[0].len();

        if frames == 0 {
            return;
        }

        output_buf.clear();
        output_buf.resize(channels*frames, 0.0f32);

        // interleave the data into output_buf
        for ch in 0..channels {
            for frame in 0..frames {
                output_buf[frame * channels + ch] = data[ch][frame];
            }
        }
        
        (data_callback)(&mut output_buf.as_slice());
    }) as Box<dyn FnMut(wasm_bindgen::JsValue)>);

    let js_func = js_closure.as_ref().unchecked_ref();

    worklet_node
        .port()
        .expect("Failed to get port")
        .set_onmessage(Some(js_func));

    Ok(ctx)
}
