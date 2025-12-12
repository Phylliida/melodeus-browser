
extern crate js_sys;
extern crate wasm_bindgen;
extern crate web_sys;

use js_sys::Array;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioContext, MediaStream, MediaStreamTrack, MediaStreamConstraints, MediaDevices, Navigator, BlobPropertyBag};
use js_sys::{Float32Array};
use cpal::SampleFormat;
use std::cell::RefCell;

/// Discovered details for a specific audio input device obtained via `getUserMedia` constraints.
#[derive(Clone, Debug)]
pub struct InputDeviceInfo {
    pub device_id: String,
    pub label: Option<String>,
    pub sample_rate: u32,
    pub channels: usize,
    pub sample_format: cpal::SampleFormat,
}


#[inline]
fn helper_log(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    #[cfg(target_arch = "wasm32")]
    {
        use js_sys::{Function, Reflect};
        let global = js_sys::global();
        let key = JsValue::from_str("logMessage");
        if let Ok(val) = Reflect::get(&global, &key) {
            if let Some(func) = val.dyn_ref::<Function>() {
                let _ = func.call1(&JsValue::NULL, &JsValue::from_str(msg));
                return;
            }
        }
        // Fallback if the JS helper isn't present.
        web_sys::console::log_1(&JsValue::from_str(msg));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        println!("{msg}");
    }
}

#[macro_export]
macro_rules! helper_log {
    ($($t:tt)*) => {
        $crate::aec::helper_log(format!($($t)*))
    };
}


pub struct WasmStream {
    audio_context: Option<web_sys::AudioContext>,
    stream: Option<web_sys::MediaStream>,
}

impl WasmStream {
    pub fn new(audio_context : web_sys::AudioContext, stream: web_sys::MediaStream) -> Self {
        Self {
            audio_context: Some(audio_context),
            stream: Some(stream),
        }
    }

    pub async fn play(&self) -> Result<(), JsErr> {
        if let Some(audio_context) = &self.audio_context {
            JsFuture::from(audio_context.resume()?).await?;
        }
        Ok(())
    }

    pub async fn pause(&self) -> Result<(), JsErr> {
        if let Some(audio_context) = &self.audio_context {
            JsFuture::from(audio_context.suspend()?).await?;
        }
        Ok(())
    }
}

// lets us auto convert JsValue to Error
#[derive(Debug, thiserror::Error)]
pub enum JsErr {
    #[error("js error: {0}")]
    Js(String),
}

impl From<JsValue> for JsErr {
    fn from(v: JsValue) -> Self {
        JsErr::Js(
            v.as_string()
            .or_else(|| js_sys::Error::from(v.clone()).message().as_string())
            .unwrap_or_else(|| format!("{v:?}"))
        )
    }
}

async fn cleanup_audio_context(context: web_sys::AudioContext) {
    match context.close() {
        Ok(val) => {
            match JsFuture::from(val).await {
                Ok(_) => {

                }
                Err(err) => {
                    let error_error = JsErr::from(err);
                    eprintln!("Cleanup wasm stream failed: {error_error}");
                }
            }
        }
        Err(err) => {
            let error_error = JsErr::from(err);
            eprintln!("Cleanup wasm stream failed: {error_error}");
        }
    }
}

impl Drop for WasmStream {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            cleanup_stream(stream);
        }

        if let Some(ctx) = self.audio_context.take() {
            wasm_bindgen_futures::spawn_local(async move { 
                cleanup_audio_context(ctx).await;
            });
        }
    }
}

fn buffer_time_step_secs(buffer_size_frames: usize, sample_rate: u32) -> f64 {
    buffer_size_frames as f64 / (sample_rate as f64)
}

thread_local! {
    // Cache the MediaDevices handle after permission is granted. Do not retain the stream; keeping
    // it alive can hold the microphone and block future getUserMedia calls.
    static INPUT_ACCESS_CACHE: RefCell<Option<MediaDevices>> = RefCell::new(None);
    static INPUT_DEVICE_CACHE: RefCell<Option<Vec<InputDeviceInfo>>> = RefCell::new(None);
}

fn cleanup_stream(stream: web_sys::MediaStream) {
    let mut tracks_to_remove = Vec::new();
    for track in stream.get_tracks().iter() {
        if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
            tracks_to_remove.push(track);
        }
    }
    for track in tracks_to_remove {
        track.stop();
        stream.remove_track(&track);
    }
}

pub async fn request_input_access() -> Result<(), JsErr> {
    helper_log("Request input access 1");
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window not available"))?;
    helper_log("Request input access 2");
    let navigator: Navigator = window.navigator();
    helper_log("Request input access 3");
    let media_devices: MediaDevices = navigator.media_devices()?;

    helper_log("Request input access 4");
    let constraints = MediaStreamConstraints::new();
    constraints.set_audio(&JsValue::from_bool(true));
    constraints.set_video(&JsValue::from_bool(false));

    helper_log("Request input access 5");
    let default_stream = media_devices.get_user_media_with_constraints(&constraints)?;
    helper_log("Request input access 6");
    let default_stream = JsFuture::from(default_stream)
        .await
        .map_err(|e| {
            helper_log(format!("getUserMedia rejected: {e:?}"));
            e
        })?;
    helper_log("Request input access 7");
    let default_stream: MediaStream = default_stream.dyn_into()?;
    helper_log("Request input access 8");
    cleanup_stream(default_stream);
    Ok(())
}

pub async fn get_webaudio_input_devices() -> Result<Vec<InputDeviceInfo>, JsErr> {
    if let Some(cached_input_devices) = INPUT_DEVICE_CACHE.with(|cell| cell.borrow().clone()) {
        return Ok(cached_input_devices);
    }
    request_input_access().await?;
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window not available"))?;
    helper_log("Requfafafaest input access 2");
    let navigator: Navigator = window.navigator();
    helper_log("Requfafafaest input access 3");
    let media_devices: MediaDevices = navigator.media_devices()?;
    // Now enumerate concrete audio input devices and probe each with its deviceId constraint.
    let devices = JsFuture::from(media_devices.enumerate_devices()?).await?;
    let devices: js_sys::Array = devices.dyn_into()?;
    let mut infos = Vec::new();
    helper_log("get_webaudio_input_devices 3");

    for device in devices.iter() {
        helper_log("get_webaudio_input_devices 4");
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
        helper_log(format!("get_webaudio_input_devices 5 {device_id}"));

        let label = js_sys::Reflect::get(&device, &JsValue::from_str("label"))
            .ok()
            .and_then(|l| l.as_string())
            .filter(|l| !l.is_empty());

        // Probe the specific device so we can grab its channel count and sample rate.
        let constraints = MediaStreamConstraints::new();
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

        helper_log("get_webaudio_input_devices 6");
        let device_stream = media_devices.get_user_media_with_constraints(&constraints)?;
        let device_stream = JsFuture::from(device_stream).await?;
        let device_stream: MediaStream = device_stream.dyn_into()?;
        helper_log("get_webaudio_input_devices 7");

        let test_context = AudioContext::new()?;
        helper_log("get_webaudio_input_devices 8");
        // Necessary to read sample rate in some browsers.
        let source = test_context.create_media_stream_source(&device_stream)?;
        helper_log("get_webaudio_input_devices 9");

        let sample_rate = test_context.sample_rate() as u32;
        let channels = source.channel_count() as usize;

        helper_log("get_webaudio_input_devices 10");
        cleanup_stream(device_stream);
        helper_log("get_webaudio_input_devices 11");
        source.disconnect()?;
        cleanup_audio_context(test_context).await;
        helper_log("get_webaudio_input_devices 12");

        let sample_format = SampleFormat::F32;

        infos.push(InputDeviceInfo {
            device_id,
            label,
            sample_rate,
            channels,
            sample_format, // wasm is always f32 sample format
        });
    }
    helper_log("get_webaudio_input_devices 13");
    INPUT_DEVICE_CACHE.with(|cell| {
        *cell.borrow_mut() = Some(infos.clone()); // clone if you still need `devices` locally
    });
    Ok(infos)
}


pub async fn build_webaudio_input_stream<D>(
    device_info: &InputDeviceInfo,
    mut data_callback: D,
) -> Result<WasmStream, JsErr>
    where
        D: FnMut(&[f32]) + Send + 'static,
{
    helper_log("Reqaaauest input access 1");
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window not available"))?;
    helper_log("Reqaaaauest input access 2");
    let navigator: Navigator = window.navigator();
    helper_log("Reqaaaauest input access 3");
    let media_devices: MediaDevices = navigator.media_devices()?;
    helper_log("make webaudio audio context 1");
    let ctx = web_sys::AudioContext::new()?;
    helper_log("make webaudio audio context 5");
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
    helper_log("make webaudio audio context 6");
    let audio_obj = js_sys::Object::new();
    let device_obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&device_obj, &JsValue::from_str("exact"), &JsValue::from_str(&device_info.device_id));
    let _ = js_sys::Reflect::set(&audio_obj, &JsValue::from_str("deviceId"), &device_obj);
    constraints.set_audio(&audio_obj.into());

    let stream = media_devices.get_user_media_with_constraints(&constraints)?;
    let stream = JsFuture::from(stream).await?;
    let stream: MediaStream = stream.dyn_into()?;
    helper_log("make webaudio audio context 7");
    
    let source = ctx.create_media_stream_source(&stream)?;

    helper_log("make webaudio audio context 8");
    // must be fetched after call to create_media_stream_source (before that, it will not be populated)
    let _sample_rate = ctx.sample_rate() as u32;

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

    helper_log("make webaudio audio context 9");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&blob_parts, &type_).unwrap();

    helper_log("make webaudio audio context 10");
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();

    helper_log("make webaudio audio context 11");
    let processor = ctx
        .audio_worklet()
        .expect("Failed to get audio worklet")
        .add_module(&url)
        .unwrap();

    helper_log("make webaudio audio context 12");
    JsFuture::from(processor).await.unwrap();

    helper_log("make webaudio audio context 13");
    web_sys::Url::revoke_object_url(&url).unwrap();

    helper_log("make webaudio audio context 14");
    let worklet_node = web_sys::AudioWorkletNode::new(ctx.as_ref(), "cpal-input-processor")
        .expect("Failed to create audio worklet node");

    helper_log("make webaudio audio context 15");
    source.connect_with_audio_node(&worklet_node).unwrap();

    helper_log("make webaudio audio context 16");
    let mut output_buf: Vec<f32> = Vec::new();

    helper_log("make webaudio audio context 17");
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
    helper_log("make webaudio audio context 18");

    let js_func = js_closure.as_ref().unchecked_ref();
    helper_log("make webaudio audio context 19");

    worklet_node
        .port()
        .expect("Failed to get port")
        .set_onmessage(Some(js_func));

    helper_log("make webaudio audio context 20");
    Ok(WasmStream::new(ctx, stream))
}
