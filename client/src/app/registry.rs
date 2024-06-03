use crate::app::{Chat, Interceptor};
use std::ops::{Deref, DerefMut};
use tcp_chat_server::entities::User;
use tcp_chat_server::proto::{registry_client::RegistryClient, UserCredentials};
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use tonic::Status;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Registry {
    pub username: String,
    pub password: String,
    client: RegistryClient<Channel>,
    editing_mode: EditingMode,
    pub failed: bool,
}

impl Registry {
    pub async fn new() -> Self {
        Self {
            editing_mode: EditingMode::default(),
            username: String::default(),
            password: String::default(),
            failed: false,
            client: {
                let tls_config = ClientTlsConfig::new()
                    .ca_certificate(Certificate::from_pem(crate::TLS_CERT))
                    .domain_name("example.com");
                let channel = Channel::from_static(crate::DEFAULT_URL)
                    .tls_config(tls_config)
                    .unwrap()
                    .connect()
                    .await
                    .unwrap();

                RegistryClient::new(channel)
            },
        }
    }

    pub const fn editing_mode(&self) -> &EditingMode {
        &self.editing_mode
    }

    pub fn toggle_mode(&mut self) {
        match &self.editing_mode {
            EditingMode::Username => self.editing_mode = EditingMode::Password,
            EditingMode::Password => self.editing_mode = EditingMode::Username,
        }
    }

    pub async fn into_chat(mut self) -> Result<Chat<Interceptor>, Status> {
        let username = self.username.clone();
        let password = self.password.clone();
        let auth_pair = self
            .login_as_user(UserCredentials { username, password })
            .await?
            .into_inner();
        let proto_uuid = auth_pair
            .user_uuid
            .ok_or_else(|| Status::invalid_argument("The server did not return a user UUID"))?;
        let uuid = Uuid::try_from(proto_uuid)
            .map_err(|_| Status::invalid_argument("The server returned an invalid user UUID"))?;
        let auth_token = auth_pair
            .token
            .ok_or_else(|| Status::invalid_argument("The server did not provide an AuthToken!"))?
            .to_string();

        self.failed = false;
        let user = User {
            uuid,
            username: self.username,
            password: self.password,
            auth_token,
        };

        Ok(Chat::new(user).await)
    }
}

impl Deref for Registry {
    type Target = RegistryClient<Channel>;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for Registry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditingMode {
    #[default]
    Username,
    Password,
}
