use crate::chat::interceptor::Interceptor;
use crate::chat::Chat;
use crate::{DEFAULT_URL, TLS_CERT};
use std::ops::{Deref, DerefMut};
use tcp_chat::entities::User;
use tcp_chat::proto::{registry_client::RegistryClient, UserCredentials};
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use uuid::Uuid;

pub struct Registry {
    client: RegistryClient<Channel>,
}

impl Registry {
    pub async fn new() -> Self {
        Self {
            client: {
                let tls_config = ClientTlsConfig::new()
                    .ca_certificate(Certificate::from_pem(TLS_CERT))
                    .domain_name("example.com");
                let channel = Channel::from_static(DEFAULT_URL)
                    .tls_config(tls_config)
                    .unwrap()
                    .connect()
                    .await
                    .unwrap();

                RegistryClient::new(channel)
            },
        }
    }

    pub async fn into_chat(
        mut self,
        credentials: UserCredentials,
    ) -> Result<Chat<Interceptor>, tonic::Status> {
        let auth_pair = self.login_as_user(credentials.clone()).await?.into_inner();
        let proto_uuid = auth_pair
            .user_uuid
            .expect("The server did not return a user UUID");
        let uuid = Uuid::try_from(proto_uuid).expect("The server returned an invalid user UUID");
        let auth_token = auth_pair
            .token
            .expect("The server did not return an AuthToken")
            .to_string();

        let user = User {
            uuid,
            username: credentials.username.clone(),
            password: credentials.password,
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
