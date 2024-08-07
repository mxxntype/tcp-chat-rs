use color_eyre::eyre;
use indexmap::IndexMap;
use ratatui::widgets::ListState;
use std::sync::Arc;
use std::time::SystemTime;
use tcp_chat_server::entities::{Message, Room, User};
use tcp_chat_server::proto::chat_client::ChatClient;
use tcp_chat_server::proto::serverside_room_event::Event::NewMessage;
use tcp_chat_server::proto::serverside_user_event::Event::AddedToRoom;
use tcp_chat_server::proto::user_lookup_request::Identifier;
use tcp_chat_server::proto::{self, UserLookupRequest};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use uuid::Uuid;

// Some named UUID types for readability.
type UserUUID = Uuid;
type RoomUUID = Uuid;
type MessageUUID = Uuid;

type Cache<K, V> = Arc<Mutex<IndexMap<K, V>>>;

#[allow(unused)]
#[derive(Debug)]
pub struct Chat<I>
where
    I: Interceptor,
{
    /// gRPC clients for easy access.
    pub(crate) client: Arc<Mutex<ChatClient<InterceptedService<Channel, I>>>>,

    /// Whether or not we have already performed a "full refresh" (on startup).
    pub(crate) refreshed: bool,

    /// The currently logged in user.
    pub(crate) user: User,

    /// An intermediate buffer to hold the message being written.
    pub(crate) message_draft: String,

    /// The UUID of the room the user currently has opened (focused).
    pub(crate) room_list_state: ListState,

    /// A list of users known to this session.
    /// Acts as a cache to avoid unnecessary lookup requests to the server.
    pub(crate) users: Cache<UserUUID, proto::User>,

    /// A list of rooms the user is a member of.
    /// Acts as a cache to avoid unnecessary lookup requests to the server.
    pub(crate) rooms: Cache<RoomUUID, Room>,

    /// A list of all messages in each room.
    /// Acts as a cache to avoid unnecessary lookup requests to the server.
    pub(crate) messages: Cache<MessageUUID, Message>,
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
            refreshed: false,
            message_draft: String::default(),
            room_list_state: ListState::default(),
            users: Arc::new(Mutex::new(IndexMap::new())),
            rooms: Arc::new(Mutex::new(IndexMap::new())),
            messages: Arc::new(Mutex::new(IndexMap::new())),
            client: {
                Arc::new(Mutex::new(ChatClient::with_interceptor(
                    channel,
                    interceptor,
                )))
            },
        }
    }
}

impl<I> Chat<I>
where
    I: Interceptor + Send + 'static,
{
    /// Fetches all the necessary data from the server and fires up event threads.
    ///
    /// # Errors
    ///
    /// This function will return an error if any errors arise while fetching the
    /// static* data over gRPC.
    ///
    /// # Panics
    ///
    /// This function will panic if a panic occurs in any of the event threads.
    pub(super) async fn load_data(&mut self) -> eyre::Result<()> {
        self.load_static_rooms().await?;
        self.user_event_thread();
        self.refreshed = true;

        Ok(())
    }

    /// Loads the rooms that the currently logged in user belongs to from the server.
    ///
    /// # Errors
    ///
    /// This function will return an error if the gRPC call fails or the server sends
    /// malformed room metadata.
    async fn load_static_rooms(&mut self) -> eyre::Result<()> {
        let rooms = self
            .client
            .lock()
            .await
            .list_rooms(())
            .await?
            .into_inner()
            .rooms;

        let mut room_cache = self.rooms.lock().await;
        room_cache.clear();
        for r in rooms {
            let untrusted_uuid = r
                .uuid
                .ok_or_else(|| eyre::eyre!("The server did not provide the room's UUID"))?;
            let uuid = Uuid::try_from(untrusted_uuid)?;

            let _ = room_cache.insert(uuid, Room { uuid, name: r.name });
            Self::load_static_messages(
                uuid,
                Arc::clone(&self.client),
                Arc::clone(&self.messages),
                Arc::clone(&self.users),
            )
            .await?;
            Self::room_event_thread(
                uuid,
                Arc::clone(&self.client),
                Arc::clone(&self.messages),
                Arc::clone(&self.users),
            );
        }
        drop(room_cache);

        Ok(())
    }

    /// Loads all static* messages for a specified room.
    ///
    /// This method is associated and requires a `client_arc` and `messages_arc` instead of
    /// acquiring these `Arc` from `self`, because doing so would render it impossible to use
    /// this method as a callback in a thread. (See above usage)
    ///
    /// *static means the ones that are already in the server's database. Messages that are being
    /// streamed to us should be added to the cache via a dedicated listening thread for each room.
    ///
    /// # Errors
    ///
    /// This function will return an error if any of the messages it receives from the gRPC server
    /// have missing or invalid fields.
    async fn load_static_messages(
        room_uuid: Uuid,
        client_arc: Arc<Mutex<ChatClient<InterceptedService<Channel, I>>>>,
        messages_arc: Cache<MessageUUID, Message>,
        users_arc: Cache<UserUUID, proto::User>,
    ) -> eyre::Result<()> {
        let messages = client_arc
            .lock()
            .await
            .list_messages(proto::Uuid::from(room_uuid))
            .await?
            .into_inner()
            .messages;

        for m in messages {
            assert_eq!(Some(proto::Uuid::from(room_uuid)), m.room_uuid);

            // Parse the message's untrused fields.
            let untrusted_message_uuid = m
                .uuid
                .ok_or_else(|| eyre::eyre!("The serverside message did not specify its `uuid`"))?;
            let message_uuid = Uuid::try_from(untrusted_message_uuid)?;
            let untrusted_sender_uuid = m.sender_uuid.ok_or_else(|| {
                eyre::eyre!("The serverside message did not specify `sender_uuid`")
            })?;
            let sender_uuid = Uuid::try_from(untrusted_sender_uuid)?;
            let unstrusted_timestamp = m.timestamp.ok_or_else(|| {
                eyre::eyre!("The serverside message's `timestamp` was not specified")
            })?;
            let timestamp = SystemTime::try_from(unstrusted_timestamp)?;

            messages_arc.lock().await.insert(
                message_uuid,
                Message {
                    uuid: message_uuid,
                    sender_uuid,
                    room_uuid,
                    text: m.text,
                    timestamp,
                },
            );

            let mut users = users_arc.lock().await;
            if users.get(&sender_uuid).is_none() {
                let _ = users.insert(
                    sender_uuid,
                    client_arc
                        .lock()
                        .await
                        .lookup_user(UserLookupRequest {
                            identifier: Some(Identifier::Uuid(sender_uuid.into())),
                        })
                        .await
                        .unwrap()
                        .into_inner(),
                );
            }
            drop(users);
        }

        Ok(())
    }

    /// Spawns an `async` task that listens for any [`ServersideUserEvent`]s and handles them accordingly.
    ///
    /// # Panics
    ///
    /// Panics if there are any errors while the subscription is active or being initiated.
    /// "errors while the subscription is active" means missing or invalid room metadata.
    fn user_event_thread(&mut self) {
        let client = Arc::clone(&self.client);
        let rooms = Arc::clone(&self.rooms);
        let messages_arc = Arc::clone(&self.messages);
        let users = Arc::clone(&self.users);

        tokio::spawn(async move {
            let mut stream = client
                .lock()
                .await
                .subscribe_to_user(())
                .await
                .expect("Could not subscribe to user events")
                .into_inner();
            while let Some(Ok(event)) = stream.next().await {
                match event
                    .event
                    .expect("The server sent an event message with no actual event inside")
                {
                    AddedToRoom(untrusted_room_uuid) => {
                        let room = client
                            .lock()
                            .await
                            .lookup_room(untrusted_room_uuid.clone())
                            .await
                            .unwrap_or_else(|_| panic!("Could not query the server about room with UUID {untrusted_room_uuid:#?}"))
                            .into_inner();
                        let uuid = room
                            .uuid
                            .expect("The server did not provide the room's UUID")
                            .try_into()
                            .expect("The server-provided room UUID is invalid");
                        rooms.lock().await.insert(
                            uuid,
                            Room {
                                uuid,
                                name: room.name,
                            },
                        );

                        Self::load_static_messages(
                            uuid,
                            Arc::clone(&client),
                            messages_arc.clone(),
                            Arc::clone(&users),
                        )
                        .await
                        .unwrap_or_else(|_| panic!("Couldn't load messages for room {uuid:?}"));
                        Self::room_event_thread(
                            uuid,
                            Arc::clone(&client),
                            Arc::clone(&messages_arc),
                            Arc::clone(&users),
                        );
                    }
                }
            }
        });
    }

    /// Spawns an `async` task that listens for [`ServersideRoomEvent`]s and handles them accordingly.
    ///
    /// # Panics
    ///
    /// Panics if there are any errors while the subscription is active or being initiated.
    /// "errors while the subscription is active" means missing or invalid message metadata.
    fn room_event_thread(
        room_uuid: Uuid,
        client: Arc<Mutex<ChatClient<InterceptedService<Channel, I>>>>,
        messages: Cache<MessageUUID, Message>,
        users: Cache<UserUUID, proto::User>,
    ) {
        tokio::spawn(async move {
            let mut stream = client
                .lock()
                .await
                .subscribe_to_room(proto::Uuid::from(room_uuid))
                .await
                .unwrap_or_else(|stat| panic!("Couldn't subscribe to room {room_uuid:?}: {stat:?}"))
                .into_inner();

            while let Some(Ok(event)) = stream.next().await {
                match event
                    .event
                    .unwrap_or_else(|| panic!("Caught an error in room event stream"))
                {
                    NewMessage(m) => {
                        assert_eq!(Some(proto::Uuid::from(room_uuid)), m.room_uuid);

                        // Parse the message's untrused fields.
                        let untrusted_message_uuid = m
                            .uuid
                            .expect("The serverside message did not specify its `uuid`");
                        let message_uuid = Uuid::try_from(untrusted_message_uuid)
                            .expect("The serverside message's UUID was invalid");
                        let untrusted_sender_uuid = m
                            .sender_uuid
                            .expect("The serverside message did not specify `sender_uuid`");
                        let sender_uuid = Uuid::try_from(untrusted_sender_uuid)
                            .expect("The serverside message's `sender_uuid` was invalid");
                        let unstrusted_timestamp = m
                            .timestamp
                            .expect("The serverside message's `timestamp` was not specified");
                        let timestamp = SystemTime::try_from(unstrusted_timestamp)
                            .expect("The serverside message's `timestamp` was invalid");

                        messages.lock().await.insert(
                            message_uuid,
                            Message {
                                uuid: message_uuid,
                                sender_uuid,
                                room_uuid,
                                text: m.text,
                                timestamp,
                            },
                        );

                        let mut users = users.lock().await;
                        if users.get(&sender_uuid).is_none() {
                            let _ = users.insert(
                                sender_uuid,
                                client
                                    .lock()
                                    .await
                                    .lookup_user(UserLookupRequest {
                                        identifier: Some(Identifier::Uuid(sender_uuid.into())),
                                    })
                                    .await
                                    .unwrap()
                                    .into_inner(),
                            );
                        }
                        drop(users);
                    }
                }
            }
        });
    }
}
