mod cli;
mod crypto;
mod lang;
mod net;
mod util;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
