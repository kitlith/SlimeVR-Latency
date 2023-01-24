mod bridge;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    bridge::recreate_client_loop().await
}
