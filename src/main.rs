mod models;
mod dnp3_service;
mod serial_proxy;
mod dnp3_frame_layer;

use axum::{
    extract::{State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;

// Native Webview Imports
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
    dpi::LogicalSize,
};
use wry::webview::WebViewBuilder;
use std::thread;
use std::sync::mpsc;

use models::*;
use serial_proxy::{start_serial_proxy_server, start_serial_proxy_client};
use dnp3_service::Dnp3Service;

#[derive(RustEmbed)]
#[folder = "frontend/"]
struct Assets;

#[derive(Clone)]
struct AppState {
    sessions: Arc<RwLock<HashMap<String, Arc<Dnp3Service>>>>,
    log_store: Arc<dnp3_service::LogStore>,
}

// Helper to get session ID from headers
fn get_session_id(headers: &HeaderMap) -> String {
    headers
        .get("X-Session-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default")
        .to_string()
}

// Helper to get/create service for session
async fn get_service(state: &AppState, session_id: &str) -> Arc<Dnp3Service> {
    let mut sessions = state.sessions.write().await;
    if let Some(service) = sessions.get(session_id) {
        return service.clone();
    }
    
    // Create new service sharing global logs
    // NOTE: This enables "Global View" logging (all tabs see all logs)
    let service = Arc::new(Dnp3Service::new(state.log_store.clone()));
    sessions.insert(session_id.to_string(), service.clone());
    service
}

fn main() {
    // 1. Create Event Loop (Must be on main thread for macOS)
    let event_loop = EventLoop::new();

    // 2. Create Window
    let window = WindowBuilder::new()
        .with_title("DNP3 Tester")
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .build(&event_loop)
        .unwrap();

    // 3. Spawn Tokio Server in Background Thread
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            run_server(tx).await;
        });
    });

    // 4. Wait for server to start and get port
    let port = rx.recv().expect("Failed to receive port from server");
    let url = format!("http://127.0.0.1:{}", port);
    println!("🖥️ WebView Loading: {}", url);

    // 5. Create WebView
    let _webview = WebViewBuilder::new(window)
        .unwrap()
        .with_url(&url)
        .unwrap()
        .with_devtools(true)
        .build()
        .unwrap();

    // 6. Run Event Loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => (),
        }
    });
}

async fn run_server(tx: mpsc::Sender<u16>) {
    // Initialize LogStore (Shared Global)
    let log_store = Arc::new(dnp3_service::LogStore::new());
    
    // Initialize Sessions Map
    let sessions = Arc::new(RwLock::new(HashMap::new()));
    
    // Initialize tracing with custom layer
    use tracing_subscriber::prelude::*;
    
    // Configure logging/tracing
    let frame_layer = dnp3_frame_layer::Dnp3FrameLayer::new(
        log_store.raw_frames.clone(),
        log_store.logs.clone(),
        log_store.frame_counter.clone(),
        log_store.log_counter.clone()
    );
    
    // Set up tracing subscriber with EnvFilter and our custom layer
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,dnp3=trace"));
    
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)  // Hide target for cleaner output
        )
        .with(filter)
        .with(frame_layer)
        .init();

    let state = AppState { sessions, log_store };

    // Auto-apply disk `default_config.json` logic using tokio::fs
    let cfg_text = match tokio::fs::read_to_string("default_config.json").await {
        Ok(s) => Some(s),
        Err(_) => match tokio::fs::read_to_string("frontend/default_config.json").await {
            Ok(s2) => Some(s2),
            Err(_) => None,
        },
    };

    if let Some(cfg_text) = cfg_text {
        match serde_json::from_str::<DeviceConfiguration>(&cfg_text) {
            Ok(dev_cfg) => {
                let svc = get_service(&state, "default").await;
                svc.update_config(dev_cfg).await;
                println!("📝 Auto-applied default_config.json to session 'default'");
            }
            Err(e) => println!("⚠️ Failed to parse default_config.json: {}", e),
        }
    }

    // Build router
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/styles.css", get(|| serve_asset("styles.css")))
        .route("/app.js", get(|| serve_asset("app.js")))
        .route("/default_config.json", get(|| serve_asset("default_config.json")))
        .route("/api/connect", post(connect_handler))
        .route("/api/serial_ports", get(serial_ports_handler))
        .route("/api/disconnect", post(disconnect_handler))
        .route("/api/config/apply", post(apply_config_handler))
        .route("/api/data", get(get_data_handler))
        .route("/api/logs", get(get_logs_handler))
        .route("/api/frames", get(get_frames_handler))
        .route("/api/host_ip", get(host_ip_handler))
        .route("/api/read", post(read_handler))
        .route("/api/control", post(control_handler))
        .route("/api/datapoints/add", post(add_datapoint_handler))
        .route("/api/datapoints/clear", post(clear_datapoints_handler))
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    // Bind to random free port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    
    println!("\n🚀 DNP3 Tester Internal Server on port {}\n", port);
    
    // Notify main thread
    tx.send(port).unwrap();

    // Start server
    axum::serve(listener, app).await.unwrap();
}

async fn index_handler() -> impl IntoResponse {
    serve_asset("index.html").await
}

async fn serve_asset(path: &str) -> Response {
    // Special-case `default_config.json` to allow overriding the embedded
    // asset by placing `default_config.json` beside the executable (CWD)
    // or in a `frontend/` folder. This lets users drop a config next to
    // the binary without embedding frontend assets into the executable.
    if path == "default_config.json" {
        // 1) Try current working directory (e.g. where the executable is run)
        if let Ok(cwd) = std::env::current_dir() {
            let p = cwd.join("default_config.json");
            if p.exists() {
                if let Ok(body) = tokio::fs::read(&p).await {
                    return Response::builder()
                        .header("content-type", "application/json")
                        .body(body.into())
                        .unwrap();
                }
            }
        }

        // 2) Fallback: try `frontend/default_config.json` for dev setups
        if let Ok(body) = tokio::fs::read("frontend/default_config.json").await {
            return Response::builder()
                .header("content-type", "application/json")
                .body(body.into())
                .unwrap();
        }
        // If neither exists, fall through to embedded asset below
    }

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let body = content.data.into_owned();
            Response::builder()
                .header("content-type", mime.as_ref())
                .body(body.into())
                .unwrap()
        }
        None => Response::builder()
            .status(404)
            .body("Not Found".into())
            .unwrap(),
    }
}

#[derive(Deserialize)]
struct ConnectRequest {
    mode: String,
    ip: String,
    port: u16,
    #[serde(rename = "localAddr")]
    local_addr: u16,
    #[serde(rename = "remoteAddr")]
    remote_addr: u16,
    #[serde(rename = "connType", default)]
    conn_type: Option<String>,
    #[serde(rename = "serialName", default)]
    serial_name: Option<String>,
    #[serde(rename = "baudRate", default)]
    baud_rate: Option<u32>,
    #[serde(rename = "dataBits", default)]
    data_bits: Option<u8>,
    #[serde(default)]
    parity: Option<String>,
    #[serde(rename = "stopBits", default)]
    stop_bits: Option<f32>,
    #[serde(default)]
    timeout: Option<u32>,
}

#[derive(Serialize)]
struct ApiResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn connect_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ConnectRequest>,
) -> Json<ApiResponse> {
    let session_id = get_session_id(&headers);
    println!("📡 Connect request [Session {}]: mode={}, {}:{}", session_id, req.mode, req.ip, req.port);

    let service = get_service(&state, &session_id).await;

    // Normalize IP: if empty, choose a sensible default depending on role.
    let ip_address = if req.ip.trim().is_empty() {
        if req.mode == "outstation" { "0.0.0.0".to_string() } else { "127.0.0.1".to_string() }
    } else {
        req.ip.clone()
    };

        // If the session has no datapoints yet, try auto-loading a default
        // config from disk (CWD `default_config.json` or `frontend/default_config.json`).
        {
            let existing = service.get_data().await;
            if existing.is_empty() {
                let cfg_text = match tokio::fs::read_to_string("default_config.json").await {
                    Ok(s) => Some(s),
                    Err(_) => match tokio::fs::read_to_string("frontend/default_config.json").await {
                        Ok(s2) => Some(s2),
                        Err(_) => None,
                    },
                };

                if let Some(cfg_text) = cfg_text {
                    if let Ok(dev_cfg) = serde_json::from_str::<DeviceConfiguration>(&cfg_text) {
                        service.update_config(dev_cfg).await;
                        println!("📝 Auto-loaded default_config.json for session {}", session_id);
                    } else {
                        println!("⚠️ Failed to parse default_config.json when auto-loading for session {}",&session_id);
                    }
                }
            }
        }

        // Determine connection type: TCP by default, allow 'serial'
        let conn_type = if let Some(ct) = &req.conn_type {
            if ct == "serial" { ConnectionType::Serial } else if ct == "tcp_server" { ConnectionType::TcpServer } else if ct == "udp" { ConnectionType::Udp } else if ct == "tls" { ConnectionType::Tls } else { ConnectionType::TcpClient }
        } else { ConnectionType::TcpClient };

        // If serial mode requested, validate the physical serial port can be opened.
        // Note: We no longer start TCP<->serial proxies here. The DNP3 service now handles serial directly.
        if conn_type == ConnectionType::Serial {
            let dev = req.serial_name.clone().unwrap_or_else(|| "".to_string());
            let baud = req.baud_rate.unwrap_or(9600);

            // Validate physical serial port can be opened before starting DNP3
            match serial_proxy::try_open_serial(&dev, baud).await {
                Ok(_) => {
                    // Serial port is available, proceed with direct serial DNP3
                    println!("✅ Serial port {} validated, proceeding with direct serial DNP3", dev);
                }
                Err(e) => {
                    println!("⚠️ Serial open failed: {}", e);
                    return Json(ApiResponse { success: false, error: Some(format!("Serial open failed: {}", e)) });
                }
            }
        }

        let config = Configuration {
        role: if req.mode == "master" {
            DeviceRole::Master
        } else {
            DeviceRole::Outstation
        },
        connection_type: conn_type,
        ip_address,
        port: req.port,
        local_address: req.local_addr,
        remote_address: req.remote_addr,
        device_config: None,
        serial_port: req.serial_name.clone(),
        baud_rate: req.baud_rate,
        data_bits: req.data_bits,
        parity: req.parity.clone(),
        stop_bits: req.stop_bits,
    };

        let result = match config.role {
            DeviceRole::Master => service.start_master(&config).await,
            DeviceRole::Outstation => service.start_outstation(&config).await,
        };

    match result {
        Ok(_) => Json(ApiResponse {
            success: true,
            error: None,
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            error: Some(e),
        }),
    }
}

async fn apply_config_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(config): Json<DeviceConfiguration>,
) -> Json<ApiResponse> {
    let session_id = get_session_id(&headers);
    println!("📝 Applying device configuration [Session {}]", session_id);
    
    let service = get_service(&state, &session_id).await;
    service.update_config(config).await;
    
    Json(ApiResponse {
        success: true,
        error: None,
    })
}

async fn disconnect_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<ApiResponse> {
    let session_id = get_session_id(&headers);
    println!("🔌 Disconnect request [Session {}]", session_id);
    
    let service = get_service(&state, &session_id).await;
    service.disconnect().await;

    Json(ApiResponse {
        success: true,
        error: None,
    })
}

#[derive(Serialize)]
struct SerializedDataPoint {
    #[serde(rename = "type")]
    point_type: String,
    index: u16,
    name: String,
    value: f64,
    quality: String,
    timestamp: i64,
}

#[derive(Serialize)]
struct Stats {
    tx: u32,
    rx: u32,
    errors: u32,
}

#[derive(Serialize)]
struct DataResponse {
    points: Vec<SerializedDataPoint>,
    stats: Stats,
    logs: Vec<String>,
}

async fn get_data_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<DataResponse> {
    let session_id = get_session_id(&headers);
    // Silent lookup: don't create service just for polling if not exists?
    // Actually, get_service creates if missing. This ensures session persistence.
    let service = get_service(&state, &session_id).await;
    
    let points = service.get_data().await;
    let stats = service.get_stats().await;
    
    let serialized_points: Vec<SerializedDataPoint> = points.iter().map(|p| {
        SerializedDataPoint {
            point_type: format!("{:?}", p.point_type),
            index: p.index,
            name: p.name.clone(),
            value: p.value,
            quality: format!("{:?}", p.quality),
            timestamp: p.timestamp.timestamp_millis(),
        }
    }).collect();
    
    Json(DataResponse {
        points: serialized_points,
        stats: Stats {
            tx: stats.tx_count,
            rx: stats.rx_count,
            errors: stats.error_count,
        },
        logs: vec![],
    })
}

// Manual read handler (Master only)
async fn read_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    let session_id = get_session_id(&headers);
    let service = get_service(&state, &session_id).await;

    match service.read_all().await {
        Ok(_) => Json(serde_json::json!({
            "success": true,
            "message": "Read completed"
        })),
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": e
        }))
    }
}

#[derive(Deserialize)]
struct ControlRequest {
    point_type: String, 
    index: u16,
    value: f64,
    #[serde(default)]
    op_mode: String,
    #[serde(default)]
    command_type: Option<String>, // "Latch" or "Pulse"
}

#[derive(Serialize)]
struct ControlResponse {
    status: String,
    message: String,
}

async fn control_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ControlRequest>,
) -> Json<ControlResponse> {
    let session_id = get_session_id(&headers);
    let service = get_service(&state, &session_id).await;
    
    // Default to Latch if not specified
    let cmd_type = req.command_type.unwrap_or_else(|| "Latch".to_string());

    println!("🎮 Control Request [Session {}]: {}[{}], Val={}, Mode={}, Type={}", session_id, req.point_type, req.index, req.value, req.op_mode, cmd_type);
    
    // Parse point type
    let point_type = match req.point_type.as_str() {
        "BinaryOutput" => DataPointType::BinaryOutput,
        "AnalogOutput" => DataPointType::AnalogOutput,
        _ => {
            return Json(ControlResponse {
                status: "error".to_string(),
                message: "Unsupported point type".to_string(),
            });
        }
    };
    
    // Execute control through DNP3
    let result = service.execute_control(point_type, req.index, req.value, req.op_mode, cmd_type).await;
    
    match result {
        Ok(msg) => Json(ControlResponse {
            status: "success".to_string(),
            message: msg,
        }),
        Err(e) => Json(ControlResponse {
            status: "error".to_string(),
            message: e,
        }),
    }
}

#[derive(Serialize)]
struct LogsResponse {
    logs: Vec<SerializedLogEntry>,
}

#[derive(Serialize)]
struct SerializedLogEntry {
    id: u64,
    timestamp: i64,
    direction: String,
    message: String,
}

async fn get_logs_handler(State(state): State<AppState>) -> Json<LogsResponse> {
    let logs = state.log_store.logs.read().await;
    
    let serialized: Vec<SerializedLogEntry> = logs.iter().map(|log| {
        SerializedLogEntry {
            id: log.id,
            timestamp: log.timestamp.timestamp_millis(),
            direction: log.direction.clone(),
            message: log.message.clone(),
        }
    }).collect();
    
    Json(LogsResponse {
        logs: serialized,
    })
}

async fn get_frames_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let frames = state.log_store.raw_frames.read().await;
    let frames_vec: Vec<_> = frames.iter().cloned().collect();
    Json(serde_json::json!({ "frames": frames_vec }))
}

async fn host_ip_handler() -> Json<serde_json::Value> {
    // Best-effort local IP detection: create an outbound UDP socket to a public IP
    // and read the local socket address. This does not send packets to the remote host.
    let ip = local_outbound_ip().unwrap_or_else(|| "".to_string());
    Json(serde_json::json!({ "ip": ip }))
}

async fn serial_ports_handler() -> Json<serde_json::Value> {
    // Best-effort cross-platform serial port listing.
    // On macOS: list /dev/cu.* and /dev/tty.*; on Linux: /dev/ttyUSB*, /dev/ttyACM*, /dev/ttyS*, /dev/ttyAMA*; on Windows use serialport::available_ports().
    let mut ports: Vec<String> = Vec::new();

    // Try to use serialport crate to get available ports (works on all platforms)
    match tokio::task::spawn_blocking(|| serialport::available_ports()).await {
        Ok(Ok(available_ports)) => {
            // Successfully got available ports
            for port in available_ports {
                ports.push(port.port_name);
            }
        }
        _ => {
            // Fallback to filesystem enumeration if serialport fails
            #[cfg(target_os = "macos")]
            {
                if let Ok(entries) = std::fs::read_dir("/dev") {
                    for e in entries.flatten() {
                        if let Ok(name) = e.file_name().into_string() {
                            if name.starts_with("cu.") || name.starts_with("tty.") {
                                ports.push(format!("/dev/{}", name));
                            }
                        }
                    }
                }
            }
            #[cfg(target_os = "linux")]
            {
                if let Ok(entries) = std::fs::read_dir("/dev") {
                    for e in entries.flatten() {
                        if let Ok(name) = e.file_name().into_string() {
                            if name.starts_with("ttyUSB") || name.starts_with("ttyACM") || name.starts_with("ttyS") || name.starts_with("ttyAMA") {
                                ports.push(format!("/dev/{}", name));
                            }
                        }
                    }
                }
            }
            #[cfg(target_os = "windows")]
            {
                // Fallback: Probe COM1..COM32 (only if serialport fails)
                for i in 1..=32u8 {
                    let com = format!("COM{}", i);
                    ports.push(com);
                }
            }
        }
    }

    // On Linux, also add filesystem enumeration results to ensure all ttyS* devices are included
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/dev") {
            for e in entries.flatten() {
                if let Ok(name) = e.file_name().into_string() {
                    let full_path = format!("/dev/{}", name);
                    if (name.starts_with("ttyUSB") || name.starts_with("ttyACM") || name.starts_with("ttyS") || name.starts_with("ttyAMA")) && !ports.contains(&full_path) {
                        ports.push(full_path);
                    }
                }
            }
        }
    }

    // Deduplicate and sort
    ports.sort();
    ports.dedup();
    Json(serde_json::json!({ "ports": ports }))
}

fn local_outbound_ip() -> Option<String> {
    // Try IPv4 first
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = sock.local_addr() {
                return Some(local_addr.ip().to_string());
            }
        }
    }
    None
}

// Add Data Point Handler
#[derive(Deserialize)]
struct AddDataPointRequest {
    point_type: String,
    index: u16,
    name: String,
}

async fn add_datapoint_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AddDataPointRequest>,
) -> Json<ApiResponse> {
    let session_id = get_session_id(&headers);
    println!("➕ Add DataPoint Request [Session {}]: {} [{}] - {}", 
        session_id, req.point_type, req.index, req.name);
    
    let service = get_service(&state, &session_id).await;
    
    // Parse point type
    let point_type = match req.point_type.as_str() {
        "BinaryInput" => DataPointType::BinaryInput,
        "BinaryOutput" => DataPointType::BinaryOutput,
        "AnalogInput" => DataPointType::AnalogInput,
        "AnalogOutput" => DataPointType::AnalogOutput,
        "Counter" => DataPointType::Counter,
        _ => {
            return Json(ApiResponse {
                success: false,
                error: Some(format!("Invalid point type: {}", req.point_type)),
            });
        }
    };
    
    match service.add_datapoint(point_type, req.index, req.name).await {
        Ok(_) => Json(ApiResponse {
            success: true,
            error: None,
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            error: Some(e),
        }),
    }
}

// Clear All Data Points Handler
async fn clear_datapoints_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<ApiResponse> {
    let session_id = get_session_id(&headers);
    println!("🗑️  Clear All DataPoints [Session {}]", session_id);
    
    let service = get_service(&state, &session_id).await;
    service.clear_datapoints().await;
    
    Json(ApiResponse {
        success: true,
        error: None,
    })
}
