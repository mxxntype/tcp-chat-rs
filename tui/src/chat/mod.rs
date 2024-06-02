pub mod interceptor;

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use tcp_chat::entities::{Message, Room, User};
use tcp_chat::proto;
use tcp_chat::proto::chat_client::ChatClient;
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Certificate, Channel, ClientTlsConfig};

// Some named UUID types for readability.
type UserUUID = uuid::Uuid;
type RoomUUID = uuid::Uuid;
type MessageUUID = uuid::Uuid;

pub struct Chat<I: Interceptor> {
    /// gRPC clients for easy access.
    client: ChatClient<InterceptedService<Channel, I>>,

    /// The currently logged in user.
    user: User,

    /// An intermediate buffer to hold the message being written.
    message_draft: String,

    /// A list of users known to this session.
    /// Acts as a cache to avoid unnecessary lookup requests to the server.
    users: HashMap<UserUUID, proto::User>,

    /// A list of rooms the user is a member of.
    /// Acts as a cache to avoid unnecessary lookup requests to the server.
    rooms: HashMap<RoomUUID, Room>,

    /// A list of all messages in each room.
    /// Acts as a cache to avoid unnecessary lookup requests to the server.
    messages: HashMap<MessageUUID, Message>,
}

impl Chat<interceptor::Interceptor> {
    pub async fn new(user: User) -> Self {
        let tls_config = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(crate::TLS_CERT))
            .domain_name("example.com");
        let channel = Channel::from_static(crate::DEFAULT_URL)
            .tls_config(tls_config)
            .expect("Incorrect TLS configuration")
            .connect()
            .await
            .expect("Could not connect to the Chat service!");
        let interceptor = interceptor::Interceptor::new(user.auth_pair());

        Self {
            user: user.clone(),
            message_draft: String::default(),
            users: HashMap::default(),
            rooms: HashMap::default(),
            messages: HashMap::default(),
            client: { ChatClient::with_interceptor(channel, interceptor) },
        }
    }
}

impl<I: Interceptor> Deref for Chat<I> {
    type Target = ChatClient<InterceptedService<Channel, I>>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl<I: Interceptor> DerefMut for Chat<I> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}
