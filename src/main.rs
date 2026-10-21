//! Minimal stand-in for the VOICEVOX Nemo engine: only the two endpoints
//! `/api/app/audio` calls (`POST /audio_query`, `POST /synthesis`) plus
//! `GET /version` for health checks. Synthesis runs in the prebuilt VOICEVOX
//! Nemo core (C API 0.15.0) with one CPU thread and only the requested
//! speaker's model loaded.

use std::ffi::{c_char, CStr, CString};
use std::sync::Mutex;

use axum::{
    body::Bytes,
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

mod ffi {
    use std::ffi::c_char;

    #[repr(C)]
    pub struct InitializeOptions {
        pub acceleration_mode: i32,
        pub cpu_num_threads: u16,
        pub load_all_models: bool,
        pub open_jtalk_dict_dir: *const c_char,
    }

    #[repr(C)]
    pub struct AudioQueryOptions {
        pub kana: bool,
    }

    #[repr(C)]
    pub struct SynthesisOptions {
        pub enable_interrogative_upspeak: bool,
    }

    extern "C" {
        pub fn voicevox_make_default_initialize_options() -> InitializeOptions;
        pub fn voicevox_initialize(options: InitializeOptions) -> i32;
        pub fn voicevox_load_model(speaker_id: u32) -> i32;
        pub fn voicevox_get_version() -> *const c_char;
        pub fn voicevox_audio_query(
            text: *const c_char,
            speaker_id: u32,
            options: AudioQueryOptions,
            output_audio_query_json: *mut *mut c_char,
        ) -> i32;
        pub fn voicevox_synthesis(
            audio_query_json: *const c_char,
            speaker_id: u32,
            options: SynthesisOptions,
            output_wav_length: *mut usize,
            output_wav: *mut *mut u8,
        ) -> i32;
        pub fn voicevox_audio_query_json_free(audio_query_json: *mut c_char);
        pub fn voicevox_wav_free(wav: *mut u8);
        pub fn voicevox_error_result_to_message(result_code: i32) -> *const c_char;
    }
}

const ACCELERATION_MODE_CPU: i32 = 1;
const RESULT_INVALID_SPEAKER_ID: i32 = 7;
const RESULT_INVALID_UTF8: i32 = 11;
const RESULT_INVALID_AUDIO_QUERY: i32 = 13;

/// The core keeps global state; calls are serialised through this lock.
static CORE: Mutex<()> = Mutex::new(());

/// AudioQuery top-level keys: engine HTTP API (camelCase) ↔ core JSON (snake_case).
/// Accent phrase and mora keys are snake_case in both.
const KEY_MAP: &[(&str, &str)] = &[
    ("speedScale", "speed_scale"),
    ("pitchScale", "pitch_scale"),
    ("intonationScale", "intonation_scale"),
    ("volumeScale", "volume_scale"),
    ("prePhonemeLength", "pre_phoneme_length"),
    ("postPhonemeLength", "post_phoneme_length"),
    ("outputSamplingRate", "output_sampling_rate"),
    ("outputStereo", "output_stereo"),
];

fn rename_keys(value: Value, to_core: bool) -> Value {
    let Value::Object(map) = value else { return value };
    let mut out = Map::with_capacity(map.len());
    for (key, v) in map {
        let renamed = KEY_MAP
            .iter()
            .find(|(api, core)| if to_core { *api == key } else { *core == key })
            .map(|(api, core)| if to_core { *core } else { *api });
        match renamed {
            Some(k) => {
                out.insert(k.to_string(), v);
            }
            // Engine-only fields the 0.15 core does not know (pauseLength,
            // pauseLengthScale) are dropped on the way in.
            None if to_core && (key == "pauseLength" || key == "pauseLengthScale") => {}
            None => {
                out.insert(key, v);
            }
        }
    }
    if !to_core {
        // Newer engines include these; null/1.0 mean "unchanged".
        out.entry("pauseLength").or_insert(Value::Null);
        out.entry("pauseLengthScale").or_insert(json!(1.0));
    }
    Value::Object(out)
}

fn core_message(code: i32) -> String {
    unsafe { CStr::from_ptr(ffi::voicevox_error_result_to_message(code)) }
        .to_string_lossy()
        .into_owned()
}

fn error(status: StatusCode, detail: impl Into<String>) -> Response {
    (status, Json(json!({ "detail": detail.into() }))).into_response()
}

fn core_error(code: i32) -> Response {
    let status = match code {
        RESULT_INVALID_SPEAKER_ID | RESULT_INVALID_UTF8 | RESULT_INVALID_AUDIO_QUERY => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error(status, core_message(code))
}

fn audio_query(text: &str, speaker: u32) -> Result<Value, Response> {
    let text = CString::new(text)
        .map_err(|_| error(StatusCode::UNPROCESSABLE_ENTITY, "text contains NUL"))?;
    let _guard = CORE.lock().unwrap();
    let mut out: *mut c_char = std::ptr::null_mut();
    let code = unsafe {
        ffi::voicevox_audio_query(text.as_ptr(), speaker, ffi::AudioQueryOptions { kana: false }, &mut out)
    };
    if code != 0 {
        return Err(core_error(code));
    }
    let parsed = serde_json::from_slice(unsafe { CStr::from_ptr(out) }.to_bytes());
    unsafe { ffi::voicevox_audio_query_json_free(out) };
    parsed
        .map(|v| rename_keys(v, false))
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn synthesis(query: Value, speaker: u32, upspeak: bool) -> Result<Vec<u8>, Response> {
    let body = CString::new(rename_keys(query, true).to_string()).unwrap();
    let _guard = CORE.lock().unwrap();
    let mut len: usize = 0;
    let mut wav: *mut u8 = std::ptr::null_mut();
    let options = ffi::SynthesisOptions { enable_interrogative_upspeak: upspeak };
    let code = unsafe { ffi::voicevox_synthesis(body.as_ptr(), speaker, options, &mut len, &mut wav) };
    if code != 0 {
        return Err(core_error(code));
    }
    let bytes = unsafe { std::slice::from_raw_parts(wav, len) }.to_vec();
    unsafe { ffi::voicevox_wav_free(wav) };
    Ok(bytes)
}

#[derive(Deserialize)]
struct AudioQueryParams {
    text: String,
    speaker: u32,
}

#[derive(Deserialize)]
struct SynthesisParams {
    speaker: u32,
    enable_interrogative_upspeak: Option<bool>,
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, Response> + Send + 'static,
) -> Result<T, Response> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

async fn post_audio_query(Query(p): Query<AudioQueryParams>) -> Response {
    match blocking(move || audio_query(&p.text, p.speaker)).await {
        Ok(v) => Json(v).into_response(),
        Err(r) => r,
    }
}

async fn post_synthesis(Query(p): Query<SynthesisParams>, body: Bytes) -> Response {
    let query: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()),
    };
    let upspeak = p.enable_interrogative_upspeak.unwrap_or(true);
    match blocking(move || synthesis(query, p.speaker, upspeak)).await {
        Ok(wav) => ([(header::CONTENT_TYPE, "audio/wav")], wav).into_response(),
        Err(r) => r,
    }
}

async fn get_version() -> Json<String> {
    let v = unsafe { CStr::from_ptr(ffi::voicevox_get_version()) };
    Json(format!("voicevox-lite {} (core {})", env!("CARGO_PKG_VERSION"), v.to_string_lossy()))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn main() {
    let dict = CString::new(env_or("OPEN_JTALK_DICT_DIR", "/opt/open_jtalk_dic_utf_8-1.11")).unwrap();
    let threads: u16 = env_or("CPU_NUM_THREADS", "1").parse().expect("CPU_NUM_THREADS");
    let speakers: Vec<u32> = env_or("SPEAKERS", "10006")
        .split(',')
        .map(|s| s.trim().parse().expect("SPEAKERS"))
        .collect();

    let mut options = unsafe { ffi::voicevox_make_default_initialize_options() };
    options.acceleration_mode = ACCELERATION_MODE_CPU;
    options.cpu_num_threads = threads;
    options.load_all_models = false;
    options.open_jtalk_dict_dir = dict.as_ptr();
    let code = unsafe { ffi::voicevox_initialize(options) };
    assert_eq!(code, 0, "voicevox_initialize: {}", core_message(code));
    for &speaker in &speakers {
        let code = unsafe { ffi::voicevox_load_model(speaker) };
        assert_eq!(code, 0, "voicevox_load_model({speaker}): {}", core_message(code));
    }

    let app = Router::new()
        .route("/audio_query", post(post_audio_query))
        .route("/synthesis", post(post_synthesis))
        .route("/version", get(get_version));

    let addr = format!("{}:{}", env_or("HOST", "0.0.0.0"), env_or("PORT", "50121"));
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .max_blocking_threads(1)
        .build()
        .unwrap()
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
            eprintln!("voicevox-lite listening on {addr} (speakers {speakers:?}, {threads} thread)");
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    use tokio::signal::unix::{signal, SignalKind};
                    let mut term = signal(SignalKind::terminate()).unwrap();
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {}
                        _ = term.recv() => {}
                    }
                })
                .await
                .unwrap();
        });
}
