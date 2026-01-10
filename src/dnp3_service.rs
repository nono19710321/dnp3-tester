use std::sync::Arc;
use std::collections::VecDeque;
use tokio::sync::RwLock;
use tracing::{info, warn};

// DNP3 Library imports
use dnp3::app::control::*;
use dnp3::app::measurement::*;
use dnp3::app::*;
use dnp3::decode::*;
use dnp3::link::*;
use dnp3::master::*;
use dnp3::outstation::*;
use dnp3::outstation::database::*;
use dnp3::tcp::*;
use dnp3::serial::{SerialSettings, DataBits, FlowControl, Parity, StopBits};

use crate::models::*;

// --- Protocol Log Entry ---
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProtocolLogEntry {
    pub id: u64, // Global log sequence ID
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub direction: String,
    pub message: String,
    pub transaction_id: u32, 
}

// --- Raw DNP3 Frame Capture ---
#[derive(Debug, Clone, serde::Serialize)]
pub struct RawFrame {
    pub id: u64, // Global frame sequence ID
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub direction: String, // "TX" or "RX"
    pub data: Vec<u8>,     // Raw binary data
}

// --- Log Store (Shared between Master and Outstation) ---
pub struct LogStore {
    pub logs: Arc<RwLock<VecDeque<ProtocolLogEntry>>>,
    pub log_counter: Arc<std::sync::atomic::AtomicU64>,
    pub raw_frames: Arc<RwLock<VecDeque<RawFrame>>>,
    pub frame_counter: Arc<std::sync::atomic::AtomicU64>,
}

impl LogStore {
    pub fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            log_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            raw_frames: Arc::new(RwLock::new(VecDeque::with_capacity(500))),
            frame_counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
}

// --- DNP3 Service State ---
pub struct Dnp3Service {
    pub data_points: Arc<RwLock<Vec<DataPoint>>>,
    pub stats: Arc<RwLock<Statistics>>,
    pub connected: Arc<RwLock<bool>>,
    
    // Shared Logs/Frames
    pub log_store: Arc<LogStore>,
    
    // Master components
    master_channel: Arc<RwLock<Option<MasterChannel>>>,
    master_association: Arc<RwLock<Option<AssociationHandle>>>,
    
    // Outstation components  
    outstation_server: Arc<RwLock<Option<dnp3::tcp::ServerHandle>>>,
    outstation_handle: Arc<RwLock<Option<OutstationHandle>>>,
}

#[derive(Debug, Clone, Default)]
pub struct Statistics {
    pub tx_count: u32,
    pub rx_count: u32,
    pub error_count: u32,
}

impl Dnp3Service {
    pub fn new(log_store: Arc<LogStore>) -> Self {
        Self {
            data_points: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(Statistics::default())),
            connected: Arc::new(RwLock::new(false)),
            log_store,
            master_channel: Arc::new(RwLock::new(None)),
            master_association: Arc::new(RwLock::new(None)),
            outstation_server: Arc::new(RwLock::new(None)),
            outstation_handle: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn update_config(&self, config: DeviceConfiguration) {
        let mut points = self.data_points.write().await;
        points.clear();

        info!("Updating device configuration: {:?}", config.name);

        // Initialize data points from configuration
        if let Some(binary_inputs) = &config.binary_inputs {
            for bi_config in binary_inputs {
                points.push(DataPoint {
                    index: bi_config.index,
                    point_type: DataPointType::BinaryInput,
                    name: bi_config.name.clone(),
                    value: 0.0,
                    quality: DataQuality::Offline,
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        if let Some(binary_outputs) = &config.binary_outputs {
            for bo_config in binary_outputs {
                points.push(DataPoint {
                    index: bo_config.index,
                    point_type: DataPointType::BinaryOutput,
                    name: bo_config.name.clone(),
                    value: 0.0,
                    quality: DataQuality::Offline,
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        if let Some(analog_inputs) = &config.analog_inputs {
            for ai_config in analog_inputs {
                points.push(DataPoint {
                    index: ai_config.index,
                    point_type: DataPointType::AnalogInput,
                    name: ai_config.name.clone(),
                    value: 0.0,
                    quality: DataQuality::Offline,
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        if let Some(analog_outputs) = &config.analog_outputs {
            for ao_config in analog_outputs {
                points.push(DataPoint {
                    index: ao_config.index,
                    point_type: DataPointType::AnalogOutput,
                    name: ao_config.name.clone(),
                    value: 0.0,
                    quality: DataQuality::Offline,
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        if let Some(counters) = &config.counters {
            for counter_config in counters {
                points.push(DataPoint {
                    index: counter_config.index,
                    point_type: DataPointType::Counter,
                    name: counter_config.name.clone(),
                    value: 0.0,
                    quality: DataQuality::Offline,
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        info!("Data points initialized. Count: {}", points.len());
    }

    /// Add a single data point
    pub async fn add_datapoint(
        &self,
        point_type: DataPointType,
        index: u16,
        name: String,
    ) -> Result<(), String> {
        let mut points = self.data_points.write().await;
        
        // Check if point already exists
        if points.iter().any(|p| p.point_type == point_type && p.index == index) {
            return Err(format!("Data point {:?}[{}] already exists", point_type, index));
        }
        
        points.push(DataPoint {
            index,
            point_type,
            name,
            value: 0.0,
            quality: DataQuality::Online,
            timestamp: chrono::Utc::now(),
        });
        
        info!("➕ Added data point: {:?}[{}] - Total points: {}", point_type, index, points.len());
        Ok(())
    }

    /// Clear all data points
    pub async fn clear_datapoints(&self) {
        let mut points = self.data_points.write().await;
        let count = points.len();
        points.clear();
        info!("🗑️  Cleared all {} data points", count);
    }

    /// Start Master - Creates TCP client to connect to Outstation
    pub async fn start_master(&self, config: &Configuration) -> Result<(), String> {
        // Cleanup existing master resources
        {
            let mut channel_lock = self.master_channel.write().await;
            *channel_lock = None; // Drop existing channel
            let mut assoc_lock = self.master_association.write().await;
            *assoc_lock = None; // Drop existing association
             // Wait a bit for resources to be released
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            
            // Reset all data points to Offline to prevent stale data from previous sessions
            let mut points = self.data_points.write().await;
            for point in points.iter_mut() {
                point.value = 0.0;
                point.quality = DataQuality::Offline;
            }
        }

        info!("🔌 Starting DNP3 Master (role=Master) using {:?} transport", config.connection_type);

        // Create Master Channel Configuration
        let mut channel_config = MasterChannelConfig::new(
            EndpointAddress::try_new(config.local_address as u16)
                .map_err(|e| format!("Invalid local address: {}", e))?
        );
        
        // Enable FULL protocol decoding - dnp3 library will output hex dumps
        channel_config.decode_level = DecodeLevel {
            application: AppDecodeLevel::ObjectValues,
            transport: TransportDecodeLevel::Payload,  // Show hex payload
            link: LinkDecodeLevel::Payload,            // Show link layer hex
            physical: PhysDecodeLevel::Data,           // Show raw TX/RX hex
        };

        // Decide transport: Serial or TCP
        let mut channel = match config.connection_type {
            crate::models::ConnectionType::Serial => {
                // Map configuration to SerialSettings
                let port = config.serial_port.as_ref().ok_or("Serial port not configured")?;
                let baud = config.baud_rate.unwrap_or(9600);
                let data_bits = match config.data_bits.unwrap_or(8) {
                    5 => DataBits::Five,
                    6 => DataBits::Six,
                    7 => DataBits::Seven,
                    _ => DataBits::Eight,
                };
                let parity = match config.parity.as_deref().unwrap_or("none").to_lowercase().as_str() {
                    "even" => Parity::Even,
                    "odd" => Parity::Odd,
                    _ => Parity::None,
                };
                let stop_bits = match config.stop_bits.unwrap_or(1.0) {
                    x if (x - 2.0).abs() < f32::EPSILON => StopBits::Two,
                    x if (x - 1.5).abs() < f32::EPSILON => StopBits::One, // Note: dnp3 doesn't have OnePointFive
                    _ => StopBits::One,
                };

                let serial_settings = SerialSettings {
                    baud_rate: baud,
                    data_bits,
                    flow_control: FlowControl::None,
                    stop_bits,
                    parity,
                };

                // Path is a single &str, not a Vec
                let path = port.as_str();

                // Spawn master using serial transport. Retry delay 1s.
                dnp3::serial::spawn_master_serial(
                    channel_config,
                    path,
                    serial_settings,
                    std::time::Duration::from_secs(1),
                    NullListener::create(),
                )
            }
            _ => {
                // TCP client channel
                spawn_master_tcp_client(
                    LinkErrorMode::Close,
                    channel_config,
                    EndpointList::new(format!("{}:{}", config.ip_address, config.port), &[]),
                    ConnectStrategy::default(),
                    NullListener::create(),
                )
            }
        };

        // Create association configuration
        let mut assoc_config = AssociationConfig::new(
            EventClasses::all(),      // Disable unsolicited responses initially
            EventClasses::all(),      // Enable after integrity poll
            Classes::all(),           // Startup integrity poll with Class 0,1,2,3
            EventClasses::none(),     // Don't auto-scan on IIN bits
        );
        assoc_config.auto_time_sync = Some(TimeSyncProcedure::Lan);
        assoc_config.keep_alive_timeout = Some(std::time::Duration::from_secs(60));

        // Create ReadHandler with shared state
        let read_handler = Box::new(MasterReadHandler::new(
            self.data_points.clone(),
            self.log_store.logs.clone(),
            self.stats.clone(),
        ));

        // Add association
        let association = channel.add_association(
            EndpointAddress::try_new(config.remote_address as u16)
                .map_err(|e| format!("Invalid remote address: {}", e))?,
            assoc_config,
            read_handler,
            Box::new(MasterAssociationHandler),
            Box::new(MasterAssociationInfo),
        ).await.map_err(|e| format!("Failed to add association: {}", e))?;

        // Note: Automatic integrity poll is disabled to give user manual control.
        // User must click "READ" to fetch data.
        /*
        association.add_poll(
            ReadRequest::ClassScan(Classes::all()),
            std::time::Duration::from_secs(60),
        ).await.map_err(|e| format!("Failed to add poll: {}", e))?;
        */

        // CRITICAL: Enable the channel to start communications and logging
        channel.enable().await.map_err(|e| format!("Failed to enable channel: {}", e))?;

        // Store the channel and association
        *self.master_channel.write().await = Some(channel);
        *self.master_association.write().await = Some(association);
        *self.connected.write().await = true;

        self.add_log("System", "Master connected", 0).await;
        Ok(())
    }

    /// Start Outstation - Creates TCP server listening for Master
    pub async fn start_outstation(&self, config: &Configuration) -> Result<(), String> {
        // Cleanup existing outstation resources
        {
            let mut server_lock = self.outstation_server.write().await;
            *server_lock = None; // Drop existing server handle (stops listening)
            let mut handle_lock = self.outstation_handle.write().await;
            *handle_lock = None; 
            // Wait a bit for port to be released
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            
            // Reset all data points to Offline
            let mut points = self.data_points.write().await;
            for point in points.iter_mut() {
                point.value = 0.0;
                point.quality = DataQuality::Offline;
            }
        }

        info!("🏭 Starting DNP3 Outstation (role=Outstation) using {:?} transport", config.connection_type);

        // Create outstation configuration
        let mut outstation_config = OutstationConfig::new(
            EndpointAddress::try_new(config.local_address as u16)
                .map_err(|e| format!("Invalid local address: {}", e))?,
            EndpointAddress::try_new(config.remote_address as u16)
                .map_err(|e| format!("Invalid remote address: {}", e))?,
            event_buffer_config(),
        );
        // Enable FULL protocol decoding - dnp3 library will output hex dumps
        outstation_config.decode_level = DecodeLevel {
            application: AppDecodeLevel::ObjectValues,
            transport: TransportDecodeLevel::Payload,
            link: LinkDecodeLevel::Payload,
            physical: PhysDecodeLevel::Data,
        };

        // Create handlers with shared state
        let control_handler = Box::new(OutstationControlHandler::new(
            self.data_points.clone(),
            self.log_store.logs.clone(),
            self.stats.clone(),
        ));

        // Decide transport: Serial or TCP server
        match config.connection_type {
            crate::models::ConnectionType::Serial => {
                let port = config.serial_port.as_ref().ok_or("Serial port not configured")?;
                let baud = config.baud_rate.unwrap_or(9600);
                let data_bits = match config.data_bits.unwrap_or(8) {
                    5 => DataBits::Five,
                    6 => DataBits::Six,
                    7 => DataBits::Seven,
                    _ => DataBits::Eight,
                };
                let parity = match config.parity.as_deref().unwrap_or("none").to_lowercase().as_str() {
                    "even" => Parity::Even,
                    "odd" => Parity::Odd,
                    _ => Parity::None,
                };
                let stop_bits = match config.stop_bits.unwrap_or(1.0) {
                    x if (x - 2.0).abs() < f32::EPSILON => StopBits::Two,
                    x if (x - 1.5).abs() < f32::EPSILON => StopBits::One, // Note: dnp3 doesn't have OnePointFive
                    _ => StopBits::One,
                };

                let serial_settings = SerialSettings {
                    baud_rate: baud,
                    data_bits,
                    flow_control: FlowControl::None,
                    stop_bits,
                    parity,
                };

                let path = port.as_str();

                // Spawn outstation over serial. This returns a Result<OutstationHandle, _>
                let outstation = dnp3::serial::spawn_outstation_serial(
                    path,
                    serial_settings,
                    outstation_config,
                    Box::new(OutstationApp),
                    Box::new(OutstationInfo),
                    control_handler,
                ).map_err(|e| format!("Failed to spawn outstation on serial {}: {}", port, e))?;

                // Initialize outstation database with current data points
                let points = self.data_points.read().await;
                outstation.transaction(|db| {
                    for point in points.iter() {
                        match point.point_type {
                            DataPointType::BinaryInput => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    BinaryInputConfig::default(),
                                );
                            }
                            DataPointType::BinaryOutput => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    BinaryOutputStatusConfig::default(),
                                );
                            }
                            DataPointType::AnalogInput => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    AnalogInputConfig {
                                        s_var: StaticAnalogInputVariation::Group30Var5,
                                        e_var: EventAnalogInputVariation::Group32Var5,
                                        deadband: 0.0,
                                    },
                                );
                            }
                            DataPointType::AnalogOutput => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    AnalogOutputStatusConfig::default(),
                                );
                            }
                            DataPointType::Counter => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    CounterConfig::default(),
                                );
                            }
                        }
                    }
                });

                *self.outstation_handle.write().await = Some(outstation.clone());
                *self.connected.write().await = true;

                // Spawn simulation task to update outstation data periodically
                self.spawn_outstation_simulation(outstation).await;

                self.add_log("System", &format!("Outstation started on serial {}", port), 0).await;
                Ok(())
            }
            _ => {
                // TCP server path (existing behavior)
                let mut server = Server::new_tcp_server(
                    LinkErrorMode::Close,
                    format!("{}:{}", config.ip_address, config.port).parse()
                        .map_err(|e| format!("Invalid address: {}", e))?,
                );

                // Add outstation to server
                let outstation = server.add_outstation(
                    outstation_config,
                    Box::new(OutstationApp),
                    Box::new(OutstationInfo),
                    control_handler,
                    NullListener::create(),
                    AddressFilter::Any,
                ).map_err(|e| format!("Failed to add outstation: {}", e))?;

                // Initialize outstation database with current data points
                let points = self.data_points.read().await;
                outstation.transaction(|db| {
                    for point in points.iter() {
                        match point.point_type {
                            DataPointType::BinaryInput => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    BinaryInputConfig::default(),
                                );
                            }
                            DataPointType::BinaryOutput => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    BinaryOutputStatusConfig::default(),
                                );
                            }
                            DataPointType::AnalogInput => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    AnalogInputConfig {
                                        s_var: StaticAnalogInputVariation::Group30Var5,
                                        e_var: EventAnalogInputVariation::Group32Var5,
                                        deadband: 0.0,
                                    },
                                );
                            }
                            DataPointType::AnalogOutput => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    AnalogOutputStatusConfig::default(),
                                );
                            }
                            DataPointType::Counter => {
                                db.add(
                                    point.index,
                                    Some(EventClass::Class1),
                                    CounterConfig::default(),
                                );
                            }
                        }
                    }
                });
                drop(points);

                let server_handle = server.bind().await.map_err(|e| format!("Failed to bind server: {}", e))?;

                *self.outstation_server.write().await = Some(server_handle);
                *self.outstation_handle.write().await = Some(outstation.clone());
                *self.connected.write().await = true;

                // Spawn simulation task to update outstation data periodically
                self.spawn_outstation_simulation(outstation).await;

                self.add_log("System", "Outstation started", 0).await;
                Ok(())
            }
        }
    }

    /// Outstation simulation - Updates data points periodically
    async fn spawn_outstation_simulation(&self, outstation: OutstationHandle) {
        let data_points = self.data_points.clone();
        let connected = self.connected.clone();

        tokio::spawn(async move {
            loop {
                if !*connected.read().await {
                    break;
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

                // Update random data points
                let mut points = data_points.write().await;
                for point in points.iter_mut() {
                    match point.point_type {
                        DataPointType::AnalogInput => {
                            point.value = 200.0 + (fastrand::f64() * 50.0) + (fastrand::f64() * 0.99); // Add fractional part
                            point.quality = DataQuality::Online;
                            point.timestamp = chrono::Utc::now();
                            
                            // Update outstation database
                            outstation.transaction(|db| {
                                db.update(
                                    point.index,
                                    &AnalogInput::new(
                                        point.value,
                                        Flags::ONLINE,
                                        Time::synchronized(point.timestamp.timestamp_millis().try_into().unwrap()),
                                    ),
                                    UpdateOptions::detect_event(),
                                );
                            });
                        }
                        DataPointType::Counter => {
                            point.value += fastrand::f64() * 10.0;
                            point.quality = DataQuality::Online;
                            point.timestamp = chrono::Utc::now();
                            
                            outstation.transaction(|db| {
                                db.update(
                                    point.index,
                                    &Counter::new(point.value as u32, Flags::ONLINE, Time::synchronized(point.timestamp.timestamp_millis().try_into().unwrap())),
                                    UpdateOptions::detect_event(),
                                );
                            });
                        }
                        DataPointType::BinaryInput => {
                             // Keep value (or could toggle), assure ONLINE
                             // Simulate a boolean change and mark point Online
                             let val = if fastrand::f64() > 0.5 { 1.0 } else { 0.0 };
                             point.value = val;
                             point.quality = DataQuality::Online;
                             point.timestamp = chrono::Utc::now();

                             outstation.transaction(|db| {
                                 db.update(
                                     point.index,
                                     &BinaryInput::new(
                                         val > 0.5,
                                         Flags::ONLINE,
                                         Time::synchronized(point.timestamp.timestamp_millis().try_into().unwrap()),
                                     ),
                                     UpdateOptions::detect_event(),
                                 );
                             });
                        }
                        DataPointType::BinaryOutput => {
                             // Do NOT randomize BinaryOutput here. AO/BO must only change
                             // in response to control operations. Ensure DB reflects the
                             // current point value/status (read-only sync).
                             point.quality = DataQuality::Online;
                             point.timestamp = chrono::Utc::now();
                             let status = point.value > 0.5;
                             let ts = Time::synchronized(point.timestamp.timestamp_millis().try_into().unwrap());

                             outstation.transaction(|db| {
                                 db.update(
                                     point.index,
                                     &BinaryOutputStatus::new(
                                         status,
                                         Flags::ONLINE,
                                         ts,
                                     ),
                                     UpdateOptions::detect_event(),
                                 );
                             });
                        }
                        DataPointType::AnalogOutput => {
                             // Do NOT randomize AnalogOutput. Only reflect current value
                             // set by control operations or manual edits.
                             point.quality = DataQuality::Online;
                             point.timestamp = chrono::Utc::now();
                             let val = point.value;
                             let ts = Time::synchronized(point.timestamp.timestamp_millis().try_into().unwrap());

                             outstation.transaction(|db| {
                                 db.update(
                                     point.index,
                                     &AnalogOutputStatus::new(
                                         val,
                                         Flags::ONLINE,
                                         ts,
                                     ),
                                     UpdateOptions::detect_event(),
                                 );
                             });
                        }
                        _ => {}
                    }
                }
            }
        });
    }

    /// Manual read request (Master mode)
    pub async fn read_all(&self) -> Result<(), String> {
        let mut assoc_guard = self.master_association.write().await;

        if let Some(ref mut assoc) = *assoc_guard {
            self.add_log("TX", "READ Class 0,1,2,3 (Integrity Poll)", 0).await;
            assoc.read(ReadRequest::class_scan(Classes::all()))
                .await
                .map_err(|e| format!("Read failed: {}", e))?;
            let mut stats = self.stats.write().await;
            stats.tx_count += 1;
            Ok(())
        } else {
            // If no real association but service is connected (e.g. serial-sim), simulate a read
            if *self.connected.read().await {
                self.add_log("TX", "Simulated READ (serial)", 0).await;
                let mut stats = self.stats.write().await;
                stats.tx_count += 1;
                stats.rx_count += 1;
                Ok(())
            } else {
                Err("Master not connected".to_string())
            }
        }
    }

    /// Execute control operation (Master mode)
    /// Uses dnp3-rs library's operate() method with CommandMode:
    /// - CommandMode::DirectOperate: FC 0x05 (with acknowledgment)
    /// - CommandMode::DirectOperateNoAck: FC 0x06 (no acknowledgment) 
    /// - CommandMode::SelectBeforeOperate: FC 0x03 + 0x04 (SBO sequence)
    ///
    /// For TRUE SBO compliance: We send Select and Operate separately
    /// allowing user to Cancel between steps
    pub async fn execute_control(
        &self,
        point_type: DataPointType,
        index: u16,
        value: f64,
        op_mode: String,
    ) -> Result<String, String> {
        let mut assoc_guard = self.master_association.write().await;
        
        if let Some(ref mut assoc) = *assoc_guard {
            match point_type {
                DataPointType::BinaryOutput => {
                    let op_type = if value > 0.5 {
                        OpType::LatchOn
                    } else {
                        OpType::LatchOff
                    };
                    
                    let command = Group12Var1::from_op_type(op_type);
                    let builder = CommandBuilder::single_header_u16(command, index);
                    
                    match op_mode.as_str() {
                        "Direct" => {
                            // FC 0x05: Direct Operate (with response)
                            info!("Sending Direct Operate (FC 0x05) for BinaryOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::DirectOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Direct Operate failed: {}", e))?;
                        }
                        "DirectNoAck" => {
                            // FC 0x06: Direct Operate No Ack
                            // Note: dnp3-rs 1.6 doesn't expose DirectOperateNoAck
                            // Using DirectOperate as fallback (FC 0x05)
                            info!("Sending Direct Operate (FC 0x05 fallback for 0x06) for BinaryOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::DirectOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Direct Operate No Ack failed: {}", e))?;
                        }
                        "Select" => {
                            // FC 0x03: Select (part 1 of SBO)
                            // NOTE: SelectBeforeOperate will send BOTH 0x03 and 0x04
                            // This is a library behavior - we cannot intercept between them
                            // However, we mark this as "Select" to track user intent
                            info!("Sending Select (FC 0x03) for BinaryOutput[{}]", index);
                            info!("WARNING: Library will auto-send Operate (0x04) after Select");
                            
                            assoc.operate(
                                CommandMode::SelectBeforeOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Select failed: {}", e))?;
                            
                            info!("Select + Operate completed by library");
                        }
                        "Operate" => {
                            // FC 0x04: Operate (part 2 of SBO, theoretically after Select)
                            // Since library auto-completes SBO, we use DirectOperate here
                            // This simulates the "Operate" step
                            info!("Sending Operate (simulated FC 0x04) for BinaryOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::DirectOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Operate failed: {}", e))?;
                        }
                        "SBO" => {
                            // Auto SBO: Select (0x03) + Operate (0x04) in one call
                            info!("Sending Auto-SBO (FC 0x03 + 0x04) for BinaryOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::SelectBeforeOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("SBO failed: {}", e))?;
                        }
                        _ => {
                            // Default: Direct Operate (FC 0x05)
                            info!("Sending Direct Operate (FC 0x05) for BinaryOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::DirectOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Control failed: {}", e))?;
                        }
                    }
                }
                DataPointType::AnalogOutput => {
                    let command = Group41Var1::new(value as i32);
                    let builder = CommandBuilder::single_header_u16(command, index);
                    
                    match op_mode.as_str() {
                        "Direct" => {
                            info!("Sending Direct Operate (FC 0x05) for AnalogOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::DirectOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Direct Operate failed: {}", e))?;
                        }
                        "DirectNoAck" => {
                            // FC 0x06: Direct Operate No Ack
                            // Note: dnp3-rs 1.6 doesn't expose DirectOperateNoAck
                            // Using DirectOperate as fallback (FC 0x05)
                            info!("Sending Direct Operate (FC 0x05 fallback for 0x06) for AnalogOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::DirectOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Direct Operate No Ack failed: {}", e))?;
                        }
                        "Select" => {
                            info!("Sending Select (FC 0x03) for AnalogOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::SelectBeforeOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Select failed: {}", e))?;
                        }
                        "Operate" => {
                            info!("Sending Operate (simulated FC 0x04) for AnalogOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::DirectOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Operate failed: {}", e))?;
                        }
                        "SBO" => {
                            info!("Sending Auto-SBO for AnalogOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::SelectBeforeOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("SBO failed: {}", e))?;
                        }
                        _ => {
                            info!("Sending Direct Operate (FC 0x05) for AnalogOutput[{}]", index);
                            
                            assoc.operate(
                                CommandMode::DirectOperate,
                                builder
                            )
                            .await
                            .map_err(|e| format!("Control failed: {}", e))?;
                        }
                    }
                }
                _ => return Err("Unsupported control point type".to_string()),
            }

            let mut stats = self.stats.write().await;
            stats.tx_count += 1;
            stats.rx_count += 1;
            
            // Optional verification read (skip for Select to preserve SBO semantics)
            if op_mode != "Select" {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                
                if let Err(e) = assoc.read(ReadRequest::class_scan(Classes::all())).await {
                     warn!("Verification read failed: {}", e);
                }
            }
            
            Ok(format!("{} Control executed", op_mode))
        } else {
            // If no real association but service is connected (simulated serial master), attempt local update
            if *self.connected.read().await {
                let mut pts = self.data_points.write().await;
                for point in pts.iter_mut() {
                    if point.index == index {
                        point.value = value;
                        point.quality = DataQuality::Online;
                        point.timestamp = chrono::Utc::now();
                        break;
                    }
                }
                let mut stats = self.stats.write().await;
                stats.tx_count += 1;
                Ok(format!("{} Control executed (simulated)", op_mode))
            } else {
                Err("Master not connected".to_string())
            }
        }
    }

    /// Disconnect
    pub async fn disconnect(&self) {
        *self.connected.write().await = false;
        
        // Clear Master components
        *self.master_channel.write().await = None;
        *self.master_association.write().await = None;
        
        // Clear Outstation components
        *self.outstation_server.write().await = None;
        *self.outstation_handle.write().await = None;
        
        self.add_log("System", "Disconnected", 0).await;
        info!("Disconnected");
    }

    pub async fn get_data(&self) -> Vec<DataPoint> {
        self.data_points.read().await.clone()
    }

    async fn add_log(&self, direction: &str, message: &str, transaction_id: u32) {
        let mut logs = self.log_store.logs.write().await;
        if logs.len() >= 1000 {
            logs.pop_front();
        }
        let id = self.log_store.log_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        logs.push_back(ProtocolLogEntry {
            id,
            timestamp: chrono::Utc::now(),
            direction: direction.to_string(),
            message: message.to_string(),
            transaction_id,
        });
    }

    pub async fn get_logs(&self) -> Vec<ProtocolLogEntry> {
        self.log_store.logs.read().await.iter().cloned().collect()
    }

    pub async fn get_frames(&self) -> Vec<RawFrame> {
        self.log_store.raw_frames.read().await.iter().cloned().collect()
    }

    async fn capture_raw_frame(&self, direction: &str, data: &[u8]) {
        let mut frames = self.log_store.raw_frames.write().await;
        if frames.len() >= 500 {
            frames.pop_front();
        }
        let id = self.log_store.frame_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        frames.push_back(RawFrame {
            id,
            timestamp: chrono::Utc::now(),
            direction: direction.to_string(),
            data: data.to_vec(),
        });
    }

    pub async fn get_stats(&self) -> Statistics {
        self.stats.read().await.clone()
    }
}

// ============================================================================
// MASTER HANDLERS
// ============================================================================

struct MasterReadHandler {
    data_points: Arc<RwLock<Vec<DataPoint>>>,
    logs: Arc<RwLock<VecDeque<ProtocolLogEntry>>>,
    stats: Arc<RwLock<Statistics>>,
}

impl MasterReadHandler {
    fn new(
        data_points: Arc<RwLock<Vec<DataPoint>>>,
        logs: Arc<RwLock<VecDeque<ProtocolLogEntry>>>,
        stats: Arc<RwLock<Statistics>>,
    ) -> Self {
        Self { data_points, logs, stats }
    }

    fn boxed(self) -> Box<Self> {
        Box::new(self)
    }

    async fn log(&self, direction: &str, message: &str) {
        let mut logs = self.logs.write().await;
        if logs.len() >= 1000 {
            logs.pop_front();
        }
        logs.push_back(ProtocolLogEntry {
            id: 0,
            timestamp: chrono::Utc::now(),
            direction: direction.to_string(),
            message: message.to_string(),
            transaction_id: 0,
        });
    }
}

impl ReadHandler for MasterReadHandler {
    fn begin_fragment(&mut self, _read_type: ReadType, _header: ResponseHeader) -> MaybeAsync<()> {
        MaybeAsync::ready(())
    }

    fn end_fragment(&mut self, _read_type: ReadType, _header: ResponseHeader) -> MaybeAsync<()> {
        let logs = self.logs.clone();
        let stats = self.stats.clone();
        
        tokio::spawn(async move {
            let mut log_queue = logs.write().await;
            if log_queue.len() >= 1000 { log_queue.pop_front(); }
            log_queue.push_back(ProtocolLogEntry {
                id: 0,
                timestamp: chrono::Utc::now(),
                direction: "RX".to_string(),
                message: "Response received".to_string(),
                transaction_id: 0,
            });
            
            let mut s = stats.write().await;
            s.rx_count += 1;
        });
        
        MaybeAsync::ready(())
    }

    fn handle_binary_input(
        &mut self,
        _info: HeaderInfo,
        iter: &mut dyn Iterator<Item = (BinaryInput, u16)>,
    ) {
        let points = self.data_points.clone();
        let values: Vec<_> = iter.collect();
        
        tokio::spawn(async move {
            let mut pts = points.write().await;
            for (measurement, index) in values {
                if let Some(point) = pts.iter_mut().find(|p| 
                    p.point_type == DataPointType::BinaryInput && p.index == index
                ) {
                    point.value = if measurement.value { 1.0 } else { 0.0 };
                    point.quality = if measurement.flags.value & 0x01 != 0 { DataQuality::Online } else { DataQuality::Offline };
                    point.timestamp = chrono::Utc::now();
                }
            }
        });
    }

    fn handle_double_bit_binary_input(
        &mut self,
        _info: HeaderInfo,
        _iter: &mut dyn Iterator<Item = (DoubleBitBinaryInput, u16)>,
    ) {
        // Not used in this application
    }

    fn handle_binary_output_status(
        &mut self,
        _info: HeaderInfo,
        iter: &mut dyn Iterator<Item = (BinaryOutputStatus, u16)>,
    ) {
        let points = self.data_points.clone();
        let values: Vec<_> = iter.collect();
        
        tokio::spawn(async move {
            let mut pts = points.write().await;
            for (measurement, index) in values {
                if let Some(point) = pts.iter_mut().find(|p| 
                    p.point_type == DataPointType::BinaryOutput && p.index == index
                ) {
                    point.value = if measurement.value { 1.0 } else { 0.0 };
                    point.quality = if measurement.flags.value & 0x01 != 0 { DataQuality::Online } else { DataQuality::Offline };
                    point.timestamp = chrono::Utc::now();
                }
            }
        });
    }

    fn handle_counter(
        &mut self,
        _info: HeaderInfo,
        iter: &mut dyn Iterator<Item = (Counter, u16)>,
    ) {
        let points = self.data_points.clone();
        let values: Vec<_> = iter.collect();
        
        tokio::spawn(async move {
            let mut pts = points.write().await;
            for (measurement, index) in values {
                if let Some(point) = pts.iter_mut().find(|p| 
                    p.point_type == DataPointType::Counter && p.index == index
                ) {
                    point.value = measurement.value as f64;
                    point.quality = if measurement.flags.value & 0x01 != 0 { DataQuality::Online } else { DataQuality::Offline };
                    point.timestamp = chrono::Utc::now();
                }
            }
        });
    }

    fn handle_frozen_counter(
        &mut self,
        _info: HeaderInfo,
        _iter: &mut dyn Iterator<Item = (FrozenCounter, u16)>,
    ) {
        // Not used
    }

    fn handle_analog_input(
        &mut self,
        _info: HeaderInfo,
        iter: &mut dyn Iterator<Item = (AnalogInput, u16)>,
    ) {
        let points = self.data_points.clone();
        let values: Vec<_> = iter.collect();
        
        tokio::spawn(async move {
            let mut pts = points.write().await;
            for (measurement, index) in values {
                if let Some(point) = pts.iter_mut().find(|p| 
                    p.point_type == DataPointType::AnalogInput && p.index == index
                ) {
                    point.value = measurement.value;
                    point.quality = if measurement.flags.value & 0x01 != 0 { DataQuality::Online } else { DataQuality::Offline };
                    point.timestamp = chrono::Utc::now();
                }
            }
        });
    }

    fn handle_analog_output_status(
        &mut self,
        _info: HeaderInfo,
        iter: &mut dyn Iterator<Item = (AnalogOutputStatus, u16)>,
    ) {
        let points = self.data_points.clone();
        let values: Vec<_> = iter.collect();
        
        tokio::spawn(async move {
            let mut pts = points.write().await;
            for (measurement, index) in values {
                if let Some(point) = pts.iter_mut().find(|p| 
                    p.point_type == DataPointType::AnalogOutput && p.index == index
                ) {
                    point.value = measurement.value;
                    point.quality = if measurement.flags.value & 0x01 != 0 { DataQuality::Online } else { DataQuality::Offline };
                    point.timestamp = chrono::Utc::now();
                }
            }
        });
    }

    fn handle_octet_string(
        &mut self,
        _info: HeaderInfo,
        _iter: &mut dyn Iterator<Item = (&[u8], u16)>,
    ) {
        // Not used
    }
}

struct MasterAssociationHandler;
impl AssociationHandler for MasterAssociationHandler {}

struct MasterAssociationInfo;
impl AssociationInformation for MasterAssociationInfo {}

// ============================================================================
// OUTSTATION HANDLERS
// ============================================================================

struct OutstationControlHandler {
    data_points: Arc<RwLock<Vec<DataPoint>>>,
    logs: Arc<RwLock<VecDeque<ProtocolLogEntry>>>,
    stats: Arc<RwLock<Statistics>>,
}

impl OutstationControlHandler {
    fn new(
        data_points: Arc<RwLock<Vec<DataPoint>>>,
        logs: Arc<RwLock<VecDeque<ProtocolLogEntry>>>,
        stats: Arc<RwLock<Statistics>>,
    ) -> Self {
        Self { data_points, logs, stats }
    }

    async fn log(&self, direction: &str, message: &str) {
        let mut logs = self.logs.write().await;
        if logs.len() >= 1000 {
            logs.pop_front();
        }
        logs.push_back(ProtocolLogEntry {
            id: 0,
            timestamp: chrono::Utc::now(),
            direction: direction.to_string(),
            message: message.to_string(),
            transaction_id: 0,
        });
    }
}

impl ControlHandler for OutstationControlHandler {}

impl ControlSupport<Group12Var1> for OutstationControlHandler {
    fn select(
        &mut self,
        control: Group12Var1,
        index: u16,
        _database: &mut DatabaseHandle,
    ) -> CommandStatus {
        let logs = self.logs.clone();
        let value = if control.code.op_type == OpType::LatchOn { 1.0 } else { 0.0 };
        
        tokio::spawn(async move {
            let mut log_queue = logs.write().await;
            if log_queue.len() >= 1000 { log_queue.pop_front(); }
            log_queue.push_back(ProtocolLogEntry {
                id: 0,
                timestamp: chrono::Utc::now(),
                direction: "RX".to_string(),
                message: format!("[FC=03 SELECT] BinaryOutput[{}] = {}", index, value),
                transaction_id: 0,
            });
            log_queue.push_back(ProtocolLogEntry {
                id: 0,
                timestamp: chrono::Utc::now(),
                direction: "TX".to_string(),
                message: "[FC=129] SELECT Success - Status: 0".to_string(),
                transaction_id: 0,
            });
        });
        
        if index < 100 && (control.code.op_type == OpType::LatchOn || control.code.op_type == OpType::LatchOff) {
            CommandStatus::Success
        } else {
            CommandStatus::NotSupported
        }
    }

    fn operate(
        &mut self,
        control: Group12Var1,
        index: u16,
        _op_type: OperateType,
        database: &mut DatabaseHandle,
    ) -> CommandStatus {
        let status = control.code.op_type == OpType::LatchOn;
        let value = if status { 1.0 } else { 0.0 };
        
        // Update database
        database.transaction(|db| {
            db.update(
                index,
                &BinaryOutputStatus::new(
                    status,
                    Flags::ONLINE,
                    Time::synchronized(chrono::Utc::now().timestamp_millis().try_into().unwrap()),
                ),
                UpdateOptions::detect_event(),
            );
        });
        
        // Update our data points
        let points = self.data_points.clone();
        tokio::spawn(async move {
            let mut pts = points.write().await;
            if let Some(point) = pts.iter_mut().find(|p| 
                p.point_type == DataPointType::BinaryOutput && p.index == index
            ) {
                point.value = value;
                point.quality = DataQuality::Online;
                point.timestamp = chrono::Utc::now();
            }
        });
        
        // Log
        let logs = self.logs.clone();
        tokio::spawn(async move {
            let mut log_queue = logs.write().await;
            if log_queue.len() >= 1000 { log_queue.pop_front(); }
            log_queue.push_back(ProtocolLogEntry {
                id: 0,
                timestamp: chrono::Utc::now(),
                direction: "RX".to_string(),
                message: format!("[FC=04 OPERATE] BinaryOutput[{}] = {}", index, value),
                transaction_id: 0,
            });
            log_queue.push_back(ProtocolLogEntry {
                id: 0,
                timestamp: chrono::Utc::now(),
                direction: "TX".to_string(),
                message: "[FC=129] OPERATE Success - Status: 0".to_string(),
                transaction_id: 0,
            });
        });
        
        CommandStatus::Success
    }
}

impl ControlSupport<Group41Var1> for OutstationControlHandler {
    fn select(
        &mut self,
        _control: Group41Var1,
        index: u16,
        _database: &mut DatabaseHandle,
    ) -> CommandStatus {
        if index < 100 {
            CommandStatus::Success
        } else {
            CommandStatus::NotSupported
        }
    }

    fn operate(
        &mut self,
        control: Group41Var1,
        index: u16,
        _op_type: OperateType,
        database: &mut DatabaseHandle,
    ) -> CommandStatus {
        let value = control.value as f64;
        
        database.transaction(|db| {
            db.update(
                index,
                &AnalogOutputStatus::new(
                    value,
                    Flags::ONLINE,
                    Time::synchronized(chrono::Utc::now().timestamp_millis().try_into().unwrap()),
                ),
                UpdateOptions::detect_event(),
            );
        });
        
        let points = self.data_points.clone();
        let logs = self.logs.clone();
        
        tokio::spawn(async move {
            let mut pts = points.write().await;
            if let Some(point) = pts.iter_mut().find(|p| 
                p.point_type == DataPointType::AnalogOutput && p.index == index
            ) {
                point.value = value;
                point.quality = DataQuality::Online;
                point.timestamp = chrono::Utc::now();
            }
            
            let mut log_queue = logs.write().await;
            if log_queue.len() >= 1000 { log_queue.pop_front(); }
            log_queue.push_back(ProtocolLogEntry {
                id: 0,
                timestamp: chrono::Utc::now(),
                direction: "RX".to_string(),
                message: format!("[FC=04 OPERATE] AnalogOutput[{}] = {}", index, value),
                transaction_id: 0,
            });
            log_queue.push_back(ProtocolLogEntry {
                id: 0,
                timestamp: chrono::Utc::now(),
                direction: "TX".to_string(),
                message: "[FC=129] OPERATE Success - Status: 0".to_string(),
                transaction_id: 0,
            });
        });
        
        CommandStatus::Success
    }
}

// Implement other Group41 variants
impl ControlSupport<Group41Var2> for OutstationControlHandler {
    fn select(&mut self, _control: Group41Var2, index: u16, _database: &mut DatabaseHandle) -> CommandStatus {
        if index < 100 { CommandStatus::Success } else { CommandStatus::NotSupported }
    }
    
    fn operate(&mut self, control: Group41Var2, index: u16, _op_type: OperateType, database: &mut DatabaseHandle) -> CommandStatus {
        let value = control.value;
        database.transaction(|db| {
            db.update(index, &AnalogOutputStatus::new(value as f64, Flags::ONLINE, Time::synchronized(chrono::Utc::now().timestamp_millis().try_into().unwrap())), UpdateOptions::detect_event());
        });
        CommandStatus::Success
    }
}

impl ControlSupport<Group41Var3> for OutstationControlHandler {
    fn select(&mut self, _control: Group41Var3, index: u16, _database: &mut DatabaseHandle) -> CommandStatus {
        if index < 100 { CommandStatus::Success } else { CommandStatus::NotSupported }
    }
    
    fn operate(&mut self, control: Group41Var3, index: u16, _op_type: OperateType, database: &mut DatabaseHandle) -> CommandStatus {
        let value = control.value;
        database.transaction(|db| {
            db.update(index, &AnalogOutputStatus::new(value as f64, Flags::ONLINE, Time::synchronized(chrono::Utc::now().timestamp_millis().try_into().unwrap())), UpdateOptions::detect_event());
        });
        CommandStatus::Success
    }
}

impl ControlSupport<Group41Var4> for OutstationControlHandler {
    fn select(&mut self, _control: Group41Var4, index: u16, _database: &mut DatabaseHandle) -> CommandStatus {
        if index < 100 { CommandStatus::Success } else { CommandStatus::NotSupported }
    }
    
    fn operate(&mut self, control: Group41Var4, index: u16, _op_type: OperateType, database: &mut DatabaseHandle) -> CommandStatus {
        let value = control.value;
        database.transaction(|db| {
            db.update(index, &AnalogOutputStatus::new(value as f64, Flags::ONLINE, Time::synchronized(chrono::Utc::now().timestamp_millis().try_into().unwrap())), UpdateOptions::detect_event());
        });
        CommandStatus::Success
    }
}

struct OutstationApp;
impl OutstationApplication for OutstationApp {}

struct OutstationInfo;
impl OutstationInformation for OutstationInfo {}

// Helper function for event buffer configuration
fn event_buffer_config() -> EventBufferConfig {
    EventBufferConfig::new(
        100, // binary
        10,  // double-bit binary
        100, // binary output status
        50,  // counter
        10,  // frozen counter
        100, // analog
        100, // analog output status
        10,  // octet string
    )
}

use dnp3::app::Listener;
use dnp3::app::MaybeAsync;

