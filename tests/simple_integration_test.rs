#[cfg(test)]
mod tests {
    use log::{error, info};
    use simple_chat::client::Client;
    use simple_chat::server::{start_server, User};
    use std::{collections::HashMap, sync::Arc};
    use tokio::net::TcpListener;
    use tokio::sync::{broadcast, Mutex};

    async fn run_server(users: Arc<Mutex<HashMap<String, User>>>) {
        println!("Starting server");
        let listener = TcpListener::bind("127.0.0.1:8081").await.unwrap();
        let (broadcast_tx, _broadcast_rx) = broadcast::channel(10);

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        info!("New client connected: {}", addr);
                        let users = Arc::clone(&users);
                        let broadcast_tx = broadcast_tx.clone();

                        tokio::spawn(async move {
                            start_server(socket, users, broadcast_tx).await;
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {:?}", e);
                    }
                }
            }
        });
    }

    #[tokio::test]
    async fn test_client_server_integration() {
        // start the server
        let users = Arc::new(Mutex::new(HashMap::new()));
        run_server(Arc::clone(&users)).await;

        // create a client
        let mut client = Client::new("127.0.0.1:8081".to_string(), "client").await;
        // wait for the client to connect
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        // check if the server received the messages
        let user_map = users.lock().await;
        assert!(user_map.contains_key("client"));
        drop(user_map);

        tokio::spawn(async move {
            client.run(true).await;
        });

        // wait for the client to connect
        tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
        let user_map_mut = users.lock().await;
        let user = user_map_mut.get("client").unwrap();
        assert_eq!(user.last_messages.len(), 2);
        assert_eq!(user.last_messages[0], "client: send hello");
        assert_eq!(user.last_messages[1], "client: send world");
        drop(user_map_mut);
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // user should be removed from the users hashmap
        let user_map_mut = users.lock().await;
        assert_eq!(user_map_mut.contains_key("client"), false);
        println!("Integration test passed!");
        std::process::exit(0);
    }
}
