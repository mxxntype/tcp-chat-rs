use registry::Registry;
use tcp_chat::proto::{user_lookup_request::Identifier, UserCredentials, UserLookupRequest};

mod chat;
mod registry;

const TLS_CERT: &str = include_str!("../../tls/ca.pem");
const DEFAULT_URL: &str = "https://luna:9001";

#[tokio::main]
async fn main() {
    let _ = color_eyre::install();

    let credentials = UserCredentials {
        username: "mxxntype".into(),
        password: "12345".into(),
    };

    let mut registry = Registry::new().await;
    let _ = registry.register_new_user(credentials.clone()).await;
    let mut chat = registry.into_chat(credentials).await.unwrap();

    let _ = chat
        .lookup_user(UserLookupRequest {
            identifier: Some(Identifier::Username("mxxntype".into())),
        })
        .await
        .unwrap();
}
