use serenity::async_trait;
use serenity::all::Message;
use serenity::all::Ready;
use serenity::prelude::*;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content == "!ping" {
            if let Err(err) = msg.reply(&ctx.http, "pong!").await {
                eprintln!("Error: {err}");
            }
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} has connected!", ready.user.name);
    }
}

#[tokio::main]
async fn main() {
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(include_str!("./../token.txt"), intents)
        .event_handler(Handler)
        .await
        .expect("Failed to create client!");

    if let Err(err) = client.start().await {
        eprintln!("Client error: {err:?}");
    }
}