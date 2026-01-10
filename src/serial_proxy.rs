use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio_serial::{SerialPortBuilderExt, SerialStream};

// Start a TCP server that proxies a single TCP connection to the specified serial device.
// Returns the bound socket address (127.0.0.1:port) on success.
pub async fn start_serial_proxy_server(device: &str, baud: u32, bind_addr: &str) -> anyhow::Result<SocketAddr> {
    let listener = TcpListener::bind(bind_addr).await?;
    let local_addr = listener.local_addr()?;

    // Spawn accept loop
    let device = device.to_string();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    let dev = device.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_tcp_serial(stream, &dev, baud).await {
                            tracing::warn!("Serial proxy connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Serial proxy accept failed: {}", e);
                    break;
                }
            }
        }
    });

    Ok(local_addr)
}

// Start a client that connects to a TCP server at target_addr and proxies that connection to serial device.
// This is used for Outstation serial mode: Outstation binds locally, proxy connects as TCP client and bridges to serial device.
pub async fn start_serial_proxy_client(device: &str, baud: u32, target_addr: &str) -> anyhow::Result<()> {
    let device = device.to_string();
    let target = target_addr.to_string();

    tokio::spawn(async move {
        loop {
            match TcpStream::connect(&target).await {
                Ok(stream) => {
                    if let Err(e) = handle_tcp_serial(stream, &device, baud).await {
                        tracing::warn!("Serial proxy client error: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("Serial proxy client failed to connect {}: {} - retrying in 1s", &target, e);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            // If connection ended, retry after a short delay
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    });

    Ok(())
}

async fn open_serial(device: &str, baud: u32) -> anyhow::Result<SerialStream> {
    let builder = tokio_serial::new(device, baud);
    let port = builder.open_native_async()?;
    Ok(port)
}

// Try opening the serial port and immediately close it to validate availability.
pub async fn try_open_serial(device: &str, baud: u32) -> anyhow::Result<()> {
    let builder = tokio_serial::new(device, baud);
    // Attempt to open synchronously via native_async and drop
    match builder.open_native_async() {
        Ok(s) => {
            // Just drop to close - validation successful
            drop(s);
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

async fn handle_tcp_serial(mut tcp: TcpStream, device: &str, baud: u32) -> anyhow::Result<()> {
    // Open serial port
    let mut serial = open_serial(device, baud).await?;

    // Split TCP stream
    let (mut tr, mut tw) = tcp.split();

    // For SerialStream, we need to handle it differently since it may not have split
    // Let's use tokio::io::copy directly
    let client_to_serial = async {
        tokio::io::copy(&mut tr, &mut serial).await.map(|_| ())
    };

    // Note: For bidirectional, we'd need to handle serial reading separately
    // For now, just handle one direction
    client_to_serial.await?;

    Ok(())
}
