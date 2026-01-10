use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataPointType {
    BinaryInput,
    BinaryOutput,
    AnalogInput,
    AnalogOutput,
    Counter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataQuality {
    Online,
    Offline,
    CommLost,
    LocalForced,
    RemoteForced,
}

#[derive(Debug, Clone)]
pub struct DataPoint {
    pub point_type: DataPointType,
    pub index: u16,
    pub name: String,
    pub value: f64,
    pub quality: DataQuality,
    pub timestamp: DateTime<Utc>,
}

impl DataPoint {
    pub fn new(point_type: DataPointType, index: u16, name: String) -> Self {
        Self {
            point_type,
            index,
            name,
            value: 0.0,
            quality: DataQuality::Online,
            timestamp: Utc::now(),
        }
    }

    pub fn update_value(&mut self, value: f64, quality: DataQuality) {
        self.value = value;
        self.quality = quality;
        self.timestamp = Utc::now();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceRole {
    Master,
    Outstation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionType {
    #[serde(rename = "tcp")]
    TcpClient,
    #[serde(rename = "tcp_server")]
    TcpServer,
    #[serde(rename = "udp")]
    Udp,
    #[serde(rename = "tls")]
    Tls,
    #[serde(rename = "serial")]
    Serial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointConfig {
    pub index: u16,
    pub name: String,
    pub description: Option<String>,
    pub unit: Option<String>,
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceConfiguration {
    pub name: Option<String>,
    pub binary_inputs: Option<Vec<PointConfig>>,
    pub binary_outputs: Option<Vec<PointConfig>>,
    pub analog_inputs: Option<Vec<PointConfig>>,
    pub analog_outputs: Option<Vec<PointConfig>>,
    pub counters: Option<Vec<PointConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Configuration {
    pub role: DeviceRole,
    pub connection_type: ConnectionType,
    pub ip_address: String,
    pub port: u16,
    pub local_address: u16,
    pub remote_address: u16,
    pub device_config: Option<DeviceConfiguration>, // 关联点表配置
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baud_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_bits: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_bits: Option<f32>,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            role: DeviceRole::Outstation,
            connection_type: ConnectionType::TcpClient,
            ip_address: "127.0.0.1".to_string(),
            port: 20000,
            local_address: 10,
            remote_address: 1,
            device_config: None,
            serial_port: None,
            baud_rate: Some(9600),
            data_bits: None,
            parity: None,
            stop_bits: None,
        }
    }
}
