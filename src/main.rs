mod app;
mod render;

#[tokio::main]
async fn main() {
    app::start::start().await;
}
