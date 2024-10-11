use log::{error, info};
use std::env;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() {
    // Initialize logger
    env_logger::init();

    // Collect command line arguments
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

    // Establish connection to the server
    let stream = match TcpStream::connect(&addr).await {
        Ok(stream) => {
            info!("Successfully connected to the server");
            stream
        }
        Err(e) => {
            error!("Failed to connect to the server: {:?}", e);
            return;
        }
    };

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Send username to server
    if let Err(e) = writer.write_all(username.as_bytes()).await {
        error!("Failed to send username to the server: {:?}", e);
        return;
    }
    info!("Sent username '{}' to the server", username);

    // Task to handle user input (stdin)
    let write_task = tokio::spawn(async move {
        let mut stdin = io::BufReader::new(io::stdin()).lines();
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

    // Task to handle receiving messages from the server
    let read_task = tokio::spawn(async move {
        loop {
            let mut buffer = String::new();

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
    });

    // Wait for either task to finish
    tokio::select! {
        _ = write_task => {
            info!("Leaving chat...");
        }
        _ = read_task => {
            info!("Disconnected");
        }
    }
}
