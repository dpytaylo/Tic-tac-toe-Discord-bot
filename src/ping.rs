use serenity::all::CommandInteraction;
use serenity::all::{CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage};
use serenity::prelude::Context;

pub fn register() -> CreateCommand {
    CreateCommand::new("ping")
        .description("Creates ephemeral message with \"pong\" text")
}

pub async fn command(ctx: Context, interaction: CommandInteraction) {
    interaction.create_response(&ctx.http, CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .ephemeral(true)
            .content("pong!")
    ))
    .await
    .expect("failed to create interaction");
}