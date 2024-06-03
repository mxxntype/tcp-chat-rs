use tcp_chat_server::TCPChat;

#[tokio::main]
async fn main() {
    TCPChat::preflight();
    let chat = TCPChat::default();
    chat.run().await;
}
