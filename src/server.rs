use log::{error, info, warn};
use rayon::prelude::*;
use std::{collections::HashMap, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};

type Tx = mpsc::UnboundedSender<String>;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct User {
    username: String,
    tx: Tx,
    pub last_messages: Vec<String>, // This will store last 2 messages for testing
}

#[tokio::main]
#[allow(dead_code)]
async fn main() {
    env_logger::init();

    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    info!("Server started on 127.0.0.1:8080");

    let users = Arc::new(Mutex::new(HashMap::new()));

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                info!("New client connected: {}", addr);
                let users = Arc::clone(&users);

                tokio::spawn(async move {
                    start_server(socket, users).await;
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {:?}", e);
            }
        }
    }
}

pub async fn start_server(socket: TcpStream, users: Arc<Mutex<HashMap<String, User>>>) {
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

    let (tx, mut rx) = mpsc::unbounded_channel();
    let user = User {
        username: username.clone(),
        tx,
        last_messages: Vec::new(),
    };
    {
        let mut users = users.lock().await;
        if users.contains_key(&username) {
            // If the username is already taken, notify the client and close the connection
            warn!("Username '{}' already taken.", username);
            writer.write_all(b"Username already taken\n").await.unwrap();
            return;
        }
        users.insert(username.clone(), user.clone());
        info!("User '{}' added to the user list.", user.username);
    }

    // Notify all users that a new user has joined the chat
    let join_msg = format!("{} has joined the chat!", username);
    // Send the join message directly to all users
    broadcast_to_others(&username, &join_msg, &users).await;

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
            // Cleanup: Remove the user from the user list when they leave
            {
                let mut users = users.lock().await;
                users.remove(&username);
            }
            break;
        }

        // Create chat message
        let chat_msg = format!("{}: {}", username, msg);
        // Send the message to all users except the sender
        broadcast_to_others(&username, &chat_msg, &users).await;

        // Save the last 2 messages for testing
        {
            let mut user = users.lock().await;
            if let Some(user) = user.get_mut(&username) {
                user.last_messages.push(chat_msg.clone());
                if user.last_messages.len() > 2 {
                    user.last_messages.drain(0..user.last_messages.len() - 2);
                }
            }
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
    broadcast_to_others(&username, &leave_msg, &users).await;

    let _ = write_task.await;
}

// New helper function to broadcast messages to other users using rayon parallel iterators
async fn broadcast_to_others(
    sender: &str,
    message: &str,
    users: &Arc<Mutex<HashMap<String, User>>>,
) {
    let users = users.lock().await;
    users.par_iter().for_each(|(username, user)| {
        if username != sender {
            let _ = user.tx.send(message.to_string() + "\n");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_add_user() {
        let users = Arc::new(Mutex::new(HashMap::new()));
        let (tx, _rx) = mpsc::unbounded_channel();
        let user = User {
            username: "test_user".to_string(),
            tx,
            last_messages: Vec::new(),
        };

        {
            let mut users_lock = users.lock().await;
            users_lock.insert("test_user".to_string(), user.clone());
            assert!(users_lock.contains_key("test_user"));
        }
    }
}
