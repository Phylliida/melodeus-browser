
extern crate js_sys;
extern crate wasm_bindgen;
extern crate web_sys;

use cpal::SampleRate;
use js_sys::Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, MediaDevices, MediaStream, MediaStreamConstraints, MediaStreamTrack, Navigator,
};


use self::wasm_bindgen::prelude::*;
use self::wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use self::web_sys::{AudioContext, AudioContextOptions, MediaStream, MediaStreamConstraints, MediaDevices, Navigator, BlobPropertyBag};
use js_sys::{Float32Array};
use cpal::{
    BackendSpecificError, BufferSize, BuildStreamError, Data, DefaultStreamConfigError,
    DeviceDescription, DeviceDescriptionBuilder, DeviceId, DeviceIdError, DeviceNameError,
    DevicesError, InputCallbackInfo, OutputCallbackInfo, PauseStreamError, PlayStreamError,
    SampleFormat, SampleRate, StreamConfig, StreamError, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, SupportedStreamConfigsError,
    StreamInstant,Stream,SampleRate,
};
use std::ops::DerefMut;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;


/// Discovered details for a specific audio input device obtained via `getUserMedia` constraints.
#[derive(Clone, Debug)]
pub struct InputDeviceInfo {
    pub device_id: String,
    pub label: Option<String>,
    pub sample_rate: SampleRate,
    pub channels: usize,
}
fn buffer_time_step_secs(buffer_size_frames: usize, sample_rate: SampleRate) -> f64 {
    buffer_size_frames as f64 / sample_rate as f64
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

        let sample_rate = SampleRate(test_context.sample_rate() as u32);
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
) -> Result<Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
{
    Ok(build_input_stream_raw(device_info, data_callback, error_callback).await.map_err(
        |err| -> BuildStreamError {
            let description = format!("{:?}", err);
            let err = BackendSpecificError { description };
            err.into()
        },
    )?)
}

pub async fn build_input_stream_raw<D, E>(
    device_info: InputDeviceInfo,
    mut data_callback: D,
    error_callback: E
) -> Result<Stream, JsValue>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
{

    let ctx = web_sys::AudioContext::new()?;
    // SAFETY: WASM is single-threaded, so Arc is safe even though AudioContext is not Send/Sync
    #[allow(clippy::arc_with_non_send_sync)]
    let ctx = Arc::new(ctx);
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
    let sample_rate = ctx.sample_rate() as SampleRate;

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

    // web audio always f32
    let sample_format = SampleFormat::F32;

    let mut time_at_start_of_buffer = 0.0f64;
    // Float32Array
    let ctx_for_callback = ctx.clone();
    let js_closure = Closure::wrap(Box::new(move |msg: wasm_bindgen::JsValue| {
        let now = ctx_for_callback.current_time();
        
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

        let ptr = output_buf.as_mut_ptr() as *mut ();
        let mut data = unsafe { Data::from_parts(ptr, output_buf.len(), sample_format) };

        time_at_start_of_buffer = {
            // Synchronise first buffer as necessary (eg. keep the time value
            // referenced to the context clock).
            if time_at_start_of_buffer > 0.001 {
                time_at_start_of_buffer
            } else {
                // 25ms of time to fetch the first sample data, increase to avoid
                // initial underruns.
                now + 0.025
            }
        };
        
        let callback = crate::StreamInstant::from_secs_f64(now);
        let capture = crate::StreamInstant::from_secs_f64(time_at_start_of_buffer);
        let buffer_time_step_secs = buffer_time_step_secs(frames, sample_rate);
        time_at_start_of_buffer += buffer_time_step_secs;
        
        let timestamp = crate::InputStreamTimestamp { callback, capture };
        let info = InputCallbackInfo { timestamp };
        (data_callback)(&mut data, &info);
    }) as Box<dyn FnMut(wasm_bindgen::JsValue)>);

    let js_func = js_closure.as_ref().unchecked_ref();

    worklet_node
        .port()
        .expect("Failed to get port")
        .set_onmessage(Some(js_func));

    let stream_config = StreamConfig {
        channels: device_info.channels as u16,
        sample_rate,
        buffer_size: BufferSize::Default,
    };

    Ok(Stream {
        ctx,
        on_ended_closures: Vec::new(),
        config: stream_config,
        buffer_size_frames: 10,
        _input_onmessage_closure: Some(js_closure),
    })
}
