mod app;

use crate::app::App;
use std::io;

const TLS_CERT: &str = include_str!("../../tls/ca.pem");
const DEFAULT_URL: &str = "https://luna:9001";

#[tokio::main]
#[allow(clippy::significant_drop_tightening)]
async fn main() -> io::Result<()> {
    let _ = color_eyre::install();

    let mut app = App::new().await;
    app.run().await?;

    Ok(())
}
