use std::{
    collections::HashMap,
    env,
    sync::Arc,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, RwLock},
};
use tokio_tungstenite::{
    accept_async,
    tungstenite::{protocol::Message, Error},
};

use futures::{Stream, Sink, stream::StreamExt, sink::SinkExt};

type UserId = usize;
type Sender = mpsc::UnboundedSender<Message>;
type Receiver = mpsc::UnboundedReceiver<Message>;
type Clients = Arc<RwLock<HashMap<UserId, Sender>>>;

struct ChatServer {
    clients: Clients,
}

impl ChatServer {
    fn new() -> Self {
        ChatServer {
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn run(&self, socket: TcpStream, user_id: UserId) {
        log::info!("User {} connected", user_id);

        if let Ok(ws_stream) = accept_async(socket).await {
            let (client_sender, client_receiver) = ws_stream.split();
            let (sender, receiver) = mpsc::unbounded_channel();

            {
                let mut clients = self.clients.write().await;
                clients.insert(user_id, sender);
            }

            let client_read = read_messages(client_receiver, user_id, self.clients.clone());
            let client_write = write_messages(client_sender, receiver);

            tokio::select! {
                _ = client_read => (),
                _ = client_write => (),
            }

            {
                let mut clients = self.clients.write().await;
                clients.remove(&user_id);
            }
            log::info!("User {} disconnected", user_id);
        } else {
            log::error!("Error during WebSocket handshake with user {}", user_id);
        }
    }
}

async fn read_messages(
    mut client_receiver: impl Stream<Item = Result<Message, Error>> + Unpin,
    user_id: UserId,
    clients: Clients,
) {
    while let Some(message) = client_receiver.next().await {
        match message {
            Ok(msg) => {
                log::info!("Received a message from user {}: {:?}", user_id, msg);
                let clients = clients.read().await;
                for (&client_id, sender) in clients.iter() {
                    if client_id != user_id {
                        if let Err(e) = sender.send(msg.clone()) {
                            log::error!("Error sending message to user {}: {}", client_id, e);
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("Error receiving message from user {}: {}", user_id, e);
                break;
            }
        }
    }
}

async fn write_messages(
    mut client_sender: impl Sink<Message, Error = Error> + Unpin,
    mut receiver: Receiver,
) {
    while let Some(message) = receiver.recv().await {
        if let Err(e) = client_sender.send(message).await {
            log::error!("Error sending message to WebSocket: {}", e);
            break;
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let addr = env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    log::info!("Listening on: {}", addr);

    let chat_server = Arc::new(ChatServer::new());
    let mut user_id = 0;

    while let Ok((stream, _)) = listener.accept().await {
        let server = chat_server.clone();
        tokio::spawn(async move {
            server.run(stream, user_id).await;
        });
        user_id += 1;
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
    use futures::{SinkExt, StreamExt};
    use std::str::FromStr;
    use tokio::sync::mpsc;
    use tokio::net::TcpListener;

    async fn setup_websocket_client(addr: &str) -> (impl SinkExt<Message> + Unpin, impl StreamExt<Item = Result<Message, Error>> + Unpin) {
        let url = url::Url::from_str(addr).unwrap();
        let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
        ws_stream.split()
    }

    async fn start_test_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let chat_server = Arc::new(ChatServer::new());
            while let Ok((stream, _)) = listener.accept().await {
                let server = chat_server.clone();
                tokio::spawn(async move {
                    server.run(stream, 0).await;
                });
            }
        });
        local_addr
    }

    #[tokio::test]
    async fn test_message_broadcast() {
        // Start the chat server and get its address
        let server_addr = start_test_server().await;
        let ws_addr = format!("ws://{}", server_addr);

        // Set up two WebSocket clients simulating `websocat`
        let (mut client1_write, mut client1_read) = setup_websocket_client(&ws_addr).await;
        let (mut client2_write, mut client2_read) = setup_websocket_client(&ws_addr).await;
    
        // Wait for a moment to ensure the server is ready
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Create channels to receive messages from clients
        let (tx1, mut rx1) = mpsc::unbounded_channel();
        let (tx2, mut rx2) = mpsc::unbounded_channel();

        // Read messages in separate tasks
        tokio::spawn(async move {
            while let Some(Ok(msg)) = client1_read.next().await {
                tx1.send(msg).unwrap();
            }
        });

        tokio::spawn(async move {
            while let Some(Ok(msg)) = client2_read.next().await {
                tx2.send(msg).unwrap();
            }
        });

        // Simulate sending a message from client 1
        let test_message = Message::Text("Hello from client 1".to_string());
        if let Err(_) = client1_write.send(test_message.clone()).await {
            panic!("Failed to send message from client 1");
            // Or handle the error in a way that's appropriate for your test
        }
        
        // Check if client 2 received the message
        match rx2.recv().await {
            Some(Message::Text(text)) => assert_eq!(text, "Hello from client 1"),
            _ => panic!("Client 2 did not receive the correct message"),
        }

        // Ensure client 1 did not receive its own message
        assert!(rx1.try_recv().is_err());
    }
}
