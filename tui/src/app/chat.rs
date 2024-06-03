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

#[derive(Debug)]
pub struct Chat<I>
where
    I: Interceptor,
{
    /// gRPC clients for easy access.
    pub(crate) client: ChatClient<InterceptedService<Channel, I>>,

    /// The currently logged in user.
    pub(crate) user: User,

    /// An intermediate buffer to hold the message being written.
    pub(crate) message_draft: String,

    /// A list of users known to this session.
    /// Acts as a cache to avoid unnecessary lookup requests to the server.
    pub(crate) users: HashMap<UserUUID, proto::User>,

    /// A list of rooms the user is a member of.
    /// Acts as a cache to avoid unnecessary lookup requests to the server.
    pub(crate) rooms: HashMap<RoomUUID, Room>,

    /// A list of all messages in each room.
    /// Acts as a cache to avoid unnecessary lookup requests to the server.
    pub(crate) messages: HashMap<MessageUUID, Message>,
}

impl Chat<crate::app::Interceptor> {
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
        let interceptor = crate::app::Interceptor::new(user.auth_pair());

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

impl<I> Deref for Chat<I>
where
    I: Interceptor,
{
    type Target = ChatClient<InterceptedService<Channel, I>>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl<I> DerefMut for Chat<I>
where
    I: Interceptor,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}
