use core::time;
use log::{error, info, warn};
use std::{collections::HashMap, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, Mutex};

// Type alias for the channel transmitter that sends messages to users
type Tx = mpsc::UnboundedSender<String>;

/// Represents a user in the chat with a unique username and a sender channel.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct User {
    username: String,
    tx: Tx,
}

#[tokio::main]
#[allow(dead_code)]
async fn main() {
    // Initialize the logger
    env_logger::init();

    // Bind the server to the specified address and port
    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    info!("Server started on 127.0.0.1:8080");

    // Shared state of connected users, protected by a Mutex for thread-safe access
    let users = Arc::new(Mutex::new(HashMap::new()));

    // Create a broadcast channel to send messages to all users
    let (broadcast_tx, _broadcast_rx) = broadcast::channel(10);

    // Continuously accept new client connections
    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                info!("New client connected: {}", addr);
                let users = Arc::clone(&users);
                let broadcast_tx = broadcast_tx.clone();

                // Spawn a new asynchronous task to handle each client connection
                tokio::spawn(async move {
                    handle_client(socket, users, broadcast_tx).await;
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {:?}", e);
            }
        }
    }
}

/// Handles individual client connections and their interactions with the chat server
pub async fn handle_client(
    socket: TcpStream,
    users: Arc<Mutex<HashMap<String, User>>>, // Store the User struct here
    broadcast_tx: broadcast::Sender<String>,
) {
    // Split the socket into separate reader and writer halves for bidirectional communication
    let (mut reader, mut writer) = socket.into_split();
    let mut buf = vec![0; 1024];

    // Read the username from the client
    let n = match reader.read(&mut buf).await {
        Ok(n) if n > 0 => n,
        Ok(_) => {
            warn!("Client disconnected before sending username.");
            return;
        }
        Err(e) => {
            error!("Failed to read username: {:?}", e);
            return;
        }
    };

    let username = String::from_utf8_lossy(&buf[..n]).trim_end().to_string();
    info!("User '{}' is joining the chat.", username);

    // Create a new user with a unique username
    let (tx, mut rx) = mpsc::unbounded_channel();
    let user = User {
        username: username.clone(),
        tx,
    };
    {
        let mut users = users.lock().await;
        if users.contains_key(&username) {
            // If the username is already taken, notify the client and close the connection
            warn!("Username '{}' already taken.", username);
            writer.write_all(b"Username already taken\n").await.unwrap();
            return;
        }
        // Add the user (User struct) to the shared state of connected users
        users.insert(username.clone(), user.clone());
        info!("User '{}' added to the user list.", user.username);
    }

    // Notify all users that a new user has joined the chat
    let join_msg = format!("{} has joined the chat!", username);
    if let Err(e) = broadcast_tx.send(join_msg.clone()) {
        warn!("Failed to broadcast message: {}", e);
    }
    info!("Broadcasted join message: '{}'", join_msg);

    // Spawn a task to handle sending messages from the broadcast channel to this client
    let write_task = {
        let username = username.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if let Err(e) = writer.write_all(msg.as_bytes()).await {
                    error!("Error sending message to user '{}': {:?}", username, e);
                    break;
                }
            }
        })
    };

    // Handle receiving messages from the client and broadcasting them to others
    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) => break, // Client disconnected
            Ok(n) => n,
            Err(e) => {
                error!("Error reading from user '{}': {:?}", username, e);
                break;
            }
        };

        let msg = String::from_utf8_lossy(&buf[..n]).trim().to_string();
        if msg == "leave" {
            info!("User '{}' is leaving the chat.", username);
            break;
        }

        // Broadcast the message to all connected users (except the sender)
        std::thread::sleep(time::Duration::from_secs(5));
        let chat_msg = format!("{}: {}", username, msg);
        if let Err(e) = broadcast_tx.send(chat_msg.clone()) {
            warn!("Failed to broadcast message: {}", e);
        }
        info!("Broadcasted message: '{}'", chat_msg);
    }

    // Cleanup: Remove the user from the user list when they leave
    {
        let mut users = users.lock().await;
        users.remove(&username);
        info!("User '{}' removed from the user list.", username);
    }

    // Notify all users that this user has left the chat
    let leave_msg = format!("{} has left the chat.", username);
    if let Err(e) = broadcast_tx.send(leave_msg.clone()) {
        warn!("Failed to broadcast message: {}", e);
    }
    info!("Broadcasted leave message: '{}'", leave_msg);

    // Await the write task to finish cleanly
    let _ = write_task.await;
}
