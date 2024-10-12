use log::{error, info};
use std::env;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

#[tokio::main]
#[allow(dead_code)]
async fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <HOST> <PORT> <USERNAME>", args[0]);
        return;
    }

    let host = &args[1];
    let port = &args[2];
    let username = &args[3];

    let addr = format!("{}:{}", host, port);
    info!("Attempting to connect to server at {}", addr);

    let mut client = Client::new(addr, username).await;
    client.run(false).await; // Pass true for test mode
}

#[allow(dead_code)]
pub struct Client {
    addr: String,
    username: String,
    writer: Option<tokio::net::tcp::OwnedWriteHalf>,
    reader: Option<BufReader<tokio::net::tcp::OwnedReadHalf>>,
}

impl Client {
    pub async fn new(addr: String, username: &str) -> Self {
        // Establish connection to the server
        let stream = match TcpStream::connect(&addr).await {
            Ok(stream) => {
                info!("Successfully connected to the server");
                stream
            }
            Err(e) => {
                error!("Failed to connect to the server: {:?}", e);
                panic!("Could not connect to server");
            }
        };

        let (reader, mut writer) = stream.into_split();
        let reader = BufReader::new(reader);

        // Send username to server
        if let Err(e) = writer.write_all(format!("{}\n", username).as_bytes()).await {
            error!("Failed to send username to the server: {:?}", e);
            panic!("Could not send username");
        }
        info!("Sent username '{}' to the server", username);

        Self {
            addr,
            username: username.to_string(),
            writer: Some(writer),
            reader: Some(reader),
        }
    }

    pub async fn run(&mut self, test_mode: bool) {
        let mut writer = self.writer.take().unwrap(); // Take ownership to use in closure
        let mut stdin = io::BufReader::new(io::stdin()).lines();

        // Automatically send messages in test mode
        if test_mode {
            println!("Running in test mode...");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            if let Err(e) = writer.write_all(b"send hello\n").await {
                error!("Failed to send message to server: {:?}", e);
            } else {
                info!("Sent message to server: {}", "send hello");
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            if let Err(e) = writer.write_all(b"send world\n").await {
                error!("Failed to send message to server: {:?}", e);
            } else {
                info!("Sent message to server: {}", "send world");
            }
            // sleep for a while to allow messages to be received
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            self.exit().await;
            return;
        }

        let write_task = tokio::spawn(async move {
            while let Ok(Some(line)) = stdin.next_line().await {
                if line == "leave" {
                    info!("User sent 'leave' command, disconnecting...");
                    writer.write_all(b"leave\n").await.unwrap();
                    break;
                }

                if let Err(e) = writer.write_all(line.as_bytes()).await {
                    error!("Failed to send message to server: {:?}", e);
                    break;
                }
                info!("Sent message to server: {}", line);
            }
        });

        let reader = Arc::new(Mutex::new(self.reader.take().unwrap()));
        let read_task = {
            let reader = Arc::clone(&reader);
            tokio::spawn(async move {
                loop {
                    let mut buffer = String::new();
                    let mut reader = reader.lock().await;

                    match reader.read_line(&mut buffer).await {
                        Ok(0) => {
                            info!("Server disconnected");
                            break;
                        }
                        Ok(_) => {
                            println!("{}", buffer.trim());
                            info!("Received message from server: {}", buffer.trim());
                        }
                        Err(e) => {
                            error!("Failed to read from server: {:?}", e);
                            break;
                        }
                    }
                }
            })
        };

        tokio::select! {
            _ = write_task => {
                info!("Leaving chat...");
            }
            _ = read_task => {
                info!("Disconnected");
            }
        }
    }

    pub async fn exit(&mut self) {
        if let Some(writer) = &mut self.writer {
            writer.write_all(b"leave\n").await.unwrap();
        }
    }
}
