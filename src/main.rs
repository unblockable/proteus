mod cli;
mod crypto;
mod lang;
mod net;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run().await
}
