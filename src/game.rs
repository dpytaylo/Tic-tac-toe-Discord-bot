use std::io::{BufWriter, Cursor};
use std::sync::Arc;

use image::{Rgb, ImageOutputFormat, ImageBuffer, Rgba, ColorType};
use imageproc::drawing::{draw_filled_rect_mut, Canvas};
use imageproc::rect::Rect;

use serenity::all::{CommandInteraction, ComponentInteraction, ButtonStyle};
use serenity::builder::{CreateActionRow, CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage, CreateEmbed, CreateMessage, EditInteractionResponse, CreateAttachment, CreateButton, CreateEmbedAuthor, EditMessage};
use serenity::http::Http;
use serenity::model::prelude::{UserId, Message};
use serenity::prelude::Context;

use tokio::sync::Mutex;

const BACKGROUND: Rgb<u8> = Rgb([42, 44, 47]);
const GRAY: Rgb<u8> = Rgb([232, 232, 232]);
const RED: Rgb<u8> = Rgb([196, 57, 57]);

const CELLS: [(i32, i32); 9] = [
    (0, 0),
    (100, 0),
    (200, 0),

    (0, 100),
    (100, 100),
    (200, 100),

    (0, 200),
    (100, 200),
    (200, 200),
];

#[derive(Default)]
pub struct Game {
    x_image: ImageBuffer<Rgb<u8>, Vec<u8>>,
    o_image: ImageBuffer<Rgb<u8>, Vec<u8>>,

    horizontal_scratch: ImageBuffer<Rgba<u8>, Vec<u8>>,
    vertical_scratch: ImageBuffer<Rgba<u8>, Vec<u8>>,
    diagonal_scratch_1: ImageBuffer<Rgba<u8>, Vec<u8>>, // Left to right
    diagonal_scratch_2: ImageBuffer<Rgba<u8>, Vec<u8>>, // Right to left

    new_game_canvas: ImageBuffer<Rgb<u8>, Vec<u8>>,

    wait_user: Mutex<Option<(UserId, CommandInteraction, String, Message)>>,

    sessions: Mutex<Vec<Arc<Mutex<GameSession>>>>,
}

#[derive(Clone, Copy, PartialEq)]
enum GameCell {
    None,
    First,
    Second,
}

impl Default for GameCell {
    fn default() -> Self {
        GameCell::None
    }
}

struct GameSession {
    player: (UserId, CommandInteraction, String, Message), // Third element is a name of player
    player2: (UserId, CommandInteraction, String, Option<Message>), // No message in a same channel

    stage: usize,
    cursor_pos: usize,

    map: [GameCell; 9],
    canvas: ImageBuffer<Rgb<u8>, Vec<u8>>,
}

impl Game {
    pub fn new() -> Self {
        let x_image = image::open("./resources/x.png").expect("x.png").into_rgb8();
        let o_image = image::open("./resources/o.png").expect("o.png").into_rgb8();

        let horizontal_scratch = image::open("./resources/1.png").expect("1.png").into_rgba8();
        let vertical_scratch = image::open("./resources/2.png").expect("2.png").into_rgba8();
        let diagonal_scratch_1 = image::open("./resources/3.png").expect("3.png").into_rgba8();
        let diagonal_scratch_2 = image::open("./resources/4.png").expect("4.png").into_rgba8();

        let new_game_canvas = draw_new_game_canvas();

        Self {
            x_image,
            o_image,

            horizontal_scratch,
            vertical_scratch,
            diagonal_scratch_1,
            diagonal_scratch_2,

            new_game_canvas,

            ..Default::default()
        }
    }

    pub fn register_play() -> CreateCommand {
        CreateCommand::new("play")
            .description("Start the game")
    }

    pub fn register_stop() -> CreateCommand {
        CreateCommand::new("stop")
            .description("Unimplemented")
    }

    pub async fn command(&self, ctx: Context, interaction: CommandInteraction) {
        if interaction.data.name == "stop" {
            interaction.create_response(&ctx.http, CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .content("Unimplemented!")
            ))
            .await
            .unwrap();

            return;
        }

        if self.is_player_already_in_game(&ctx.http, &interaction).await {
            return;
        }

        let (player, player2) = {
            let val = {
                self.wait_user.lock().await.take()
            };

            let name = match &interaction.member {
                Some(val) => val.nick.clone().unwrap_or_else(|| interaction.user.name.clone()),
                None => interaction.user.name.clone(),
            };

            if let Some(val) = val {
                interaction.create_response(&ctx.http, CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .ephemeral(true)
                        .embed(
                            CreateEmbed::new()
                                .title("Please, wait")
                        )
                    )
                )
                .await
                .unwrap();

                // Channel ids are unique
                if interaction.channel_id != val.1.channel_id {
                    let message = interaction.channel_id.send_message(&ctx.http, 
                        CreateMessage::new()
                            .embed(
                                CreateEmbed::new()
                                    .title(
                                        format!(
                                            "The game between {} and {} in progress!",
                                            val.2,
                                            name,
                                        )
                                    )
                            )
                    )
                    .await
                    .unwrap();

                    (
                        val,
                        (interaction.user.id, interaction, name, Some(message)),
                    )
                }
                else {
                    (
                        val,
                        (interaction.user.id, interaction, name, None),
                    )
                }
            }
            else {
                let icon_url = interaction.user.avatar_url().unwrap_or_else(||
                    interaction.user.default_avatar_url()
                );

                let message = interaction.channel_id.send_message(&ctx.http, CreateMessage::new()
                    .embed(
                        CreateEmbed::new()
                        .author(
                            CreateEmbedAuthor::new(name.clone())
                                .icon_url(icon_url)
                        )
                        .title(format!("{} wants to play tic-tac-toe game!", name))
                        .description("You can join to him/her/them by using the `/play` command.")
                    )
                )
                .await
                .unwrap();

                interaction.create_response(&ctx.http, CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .ephemeral(true)
                        .embed(
                            CreateEmbed::new()
                                .title("Please, wait for second player...")
                        )
                    )
                )
                .await
                .unwrap();

                *self.wait_user.lock().await = Some((interaction.user.id, interaction, name, message)); 
                return;
            }
        };

        let new_game = Arc::new(Mutex::new(GameSession {
            player,
            player2,

            stage: 0,
            cursor_pos: 4,

            map: Default::default(),
            canvas: self.new_game_canvas.clone(),
        }));

        {
            self.sessions.lock().await.push(Arc::clone(&new_game));
        }

        self.process_session(&ctx.http, &mut *new_game.lock().await).await;
    }

    async fn is_player_already_in_game(&self, http: &Http, interaction: &CommandInteraction) -> bool {
        let message = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .ephemeral(true)
                .embed(
                    CreateEmbed::new()
                        .title("Start a new game")
                        .description("You have already in the game. For starting a new game you should use the `/stop` command.")
                )
        );

        {
            if let Some(val) = self.wait_user.lock().await.as_ref() {
                if val.0 == interaction.user.id {
                    interaction.create_response(http, message)
                    .await
                    .unwrap();

                    return true;
                }
            }
        }

        let sessions = self.sessions.lock().await;

        for session in &*sessions {
            let session = session.lock().await;

            if session.player.0 == interaction.user.id
                || session.player2.0 == interaction.user.id
            {
                interaction.create_response(http, message)
                .await
                .unwrap();

                return true;
            }
        }

        false
    }

    async fn process_session(&self, http: &Http, session: &mut GameSession) {
        match session.stage {
            0 => {
                show_game_message(
                    http,
                    &session.player.1,
                    session.cursor_pos,
                    &session.map,
                    &session.canvas,
                ).await;

                show_wait_and_common_message(
                    http,
                    &session.player2.1,
                    &session.canvas,
                    &session.player.2,
                    &session.player2.2,
                    &mut session.player.3,
                    session.player2.3.as_mut(),
                ).await;
            }
            1 => {
                show_game_message(
                    http,
                    &session.player2.1,
                    session.cursor_pos,
                    &session.map,
                    &session.canvas,
                ).await;

                show_wait_and_common_message(
                    http,
                    &session.player.1,
                    &session.canvas,
                    &session.player.2,
                    &session.player2.2,
                    &mut session.player.3,
                    session.player2.3.as_mut(),
                ).await;
            }
            _ => unreachable!(),
        }
    }

    pub async fn component(&self, ctx: Context, component: ComponentInteraction) {
        // We are calling this because we are editing the component
        // interaction or answering to the original interaction in the progress_game()
        component.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await.unwrap();

        let original_session = self.get_current_game(&component).await.unwrap();
        let mut session = original_session.lock().await;

        match component.data.custom_id.as_str() {
            "left" => {
                if ![0, 3, 6].contains(&session.cursor_pos) {
                    session.cursor_pos -= 1;
                }

                update_game_message(&ctx.http, &component, &session).await;
            }

            "down" => {
                if session.cursor_pos <= 5 {
                    session.cursor_pos += 3
                }

                update_game_message(&ctx.http, &component, &session).await;
            }

            "up" => {
                if session.cursor_pos >= 3 {
                    session.cursor_pos -= 3
                }

                update_game_message(&ctx.http, &component, &session).await;
            }

            "right" => {
                if ![2, 5, 8].contains(&session.cursor_pos) {
                    session.cursor_pos += 1
                }

                update_game_message(&ctx.http, &component, &session).await;
            }

            "send" => {
                'condition: {
                    if component.user.id == session.player.0 {
                        if session.map[session.cursor_pos] != GameCell::None { // Unreachable in default situation
                            break 'condition;
                        }

                        let cursor_pos = session.cursor_pos;
                        session.map[cursor_pos] = GameCell::First;
                        self.draw_x(&mut session.canvas, cursor_pos);
                    }
                    else {
                        if session.map[session.cursor_pos] != GameCell::None {
                            break 'condition;
                        }

                        let cursor_pos = session.cursor_pos;
                        session.map[cursor_pos] = GameCell::Second;
                        self.draw_o(&mut session.canvas, cursor_pos);
                    }
                };

                let map = &session.map;
                
                // Checking for win
                // 0 1 2
                // 3 4 5
                // 6 7 8
                let (win_player, id) = if map[0] != GameCell::None && (map[0] == map[1]) && (map[1] == map[2]) {
                    (map[0], 0)
                }
                else if map[3] != GameCell::None && (map[3] == map[4]) && (map[4] == map[5]) {
                    (map[3], 1)
                }
                else if map[6] != GameCell::None && (map[6] == map[7]) && (map[7] == map[8]) {
                    (map[6], 2)
                }

                else if map[0] != GameCell::None && (map[0] == map[3]) && (map[3] == map[6]) {
                    (map[0], 3)
                }
                else if map[1] != GameCell::None && (map[1] == map[4]) && (map[4] == map[7]) {
                    (map[1], 4)
                }
                else if map[2] != GameCell::None && (map[2] == map[5]) && (map[5] == map[8]) {
                    (map[2], 5)
                }

                else if map[0] != GameCell::None && (map[0] == map[4]) && (map[4] == map[8]) {
                    (map[0], 6)
                }
                else if map[2] != GameCell::None && (map[2] == map[4]) && (map[4] == map[6]) {
                    (map[2], 7)
                }

                else {
                    let mut was_none = false;
                    for cell in map {
                        if *cell == GameCell::None {
                            was_none = true;
                            break;
                        }
                    }
                    
                    if !was_none {
                        let message = EditMessage::new()
                            .add_embed(CreateEmbed::new()
                                .title(
                                    format!(
                                        "The game between {} and {} has finished!",
                                        session.player.2,
                                        session.player2.2,
                                    )
                                )
                                .description("No one wins!")
                                .attachment("canvas.png")
                            )
                            .attachment(generate_attachment_rgb8(&session.canvas, "canvas.png"));

                        self.end_game_with_message(&ctx.http, &mut session, &original_session, message).await;
                        return;
                    }

                    session.stage = (session.stage + 1) % 2;
                    session.cursor_pos = 4;

                    self.process_session(&ctx.http, &mut session).await;
                    return;
                };

                let attachment = self.generate_end_attachment(&mut session, id).await;

                match win_player {
                    GameCell::First => {
                        let message = EditMessage::new()
                            .add_embed(CreateEmbed::new()
                                .title(
                                    format!(
                                        "The game between {} and {} has finished!",
                                        session.player.2,
                                        session.player2.2,
                                    )
                                )
                                .description(format!("💥 {} has won! 💥", session.player.2))
                                .attachment("canvas.png")
                            )
                            .attachment(attachment);

                        self.end_game_with_message(&ctx.http, &mut session, &original_session, message).await;
                    },
                    GameCell::Second => {
                        let message = EditMessage::new()
                            .add_embed(CreateEmbed::new()
                                .title(
                                    format!(
                                        "The game between {} and {} has finished!",
                                        session.player.2,
                                        session.player2.2,
                                    )
                                )
                                .description(format!("💥 {} has won! 💥", session.player.2))
                                .attachment("canvas.png")
                            )
                            .attachment(attachment);

                        self.end_game_with_message(&ctx.http, &mut session, &original_session, message).await;
                    },
                    GameCell::None => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }

    async fn get_current_game(&self, message_component: &ComponentInteraction) -> Option<Arc<Mutex<GameSession>>> {
        let sessions = self.sessions.lock().await;

        let mut has_game = None;
        for session in sessions.iter() {
            let session_lock = session.lock().await;
            if session_lock.player.0 == message_component.user.id || 
                session_lock.player2.0 == message_component.user.id
            {
                has_game = Some(Arc::clone(session));
            }
        }

        has_game
    }
    
    fn draw_x(&self, image: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, cell_index: usize) {
        for y in 0..80 {
            for x in 0..80 {
                image.draw_pixel(
                    CELLS[cell_index].0 as u32 + 10 + x,
                    CELLS[cell_index].1 as u32 + 10 + y,
                    *self.x_image.get_pixel(x, y),
                );
            }
        }
    }
    
    fn draw_o(&self, image: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, cell_index: usize) {
        for y in 0..80 {
            for x in 0..80 {
                image.draw_pixel(
                    CELLS[cell_index].0 as u32 + 10 + x,
                    CELLS[cell_index].1 as u32 + 10 + y,
                    *self.o_image.get_pixel(x, y),
                );
            }
        } 
    }

    async fn generate_end_attachment(&self, session: &mut GameSession, id: u32) -> CreateAttachment {        
        match id {
            0..=2 => {
                for y in 100 * id..100 * (id + 1) {
                    for x in 0..300 {
                        fill_pixel(&mut session.canvas, &self.horizontal_scratch, x, y);
                    }
                }
            }

            3..=5 => {
                for y in 0..300 { 
                    for x in 100 * (id - 3)..100 * (id - 2) {
                        fill_pixel(&mut session.canvas, &self.vertical_scratch, x, y);
                    }
                }
            }

            6 => {
                for y in 0..300 { 
                    for x in 0..300 {
                        fill_pixel(&mut session.canvas, &self.diagonal_scratch_1, x, y);
                    }
                }
            }

            7 => {
                for y in 0..300 { 
                    for x in 0..300 {
                        fill_pixel(&mut session.canvas, &self.diagonal_scratch_2, x, y);
                    }
                }
            }

            _ => unreachable!(),
        }

        generate_attachment_rgb8(&session.canvas, "canvas.png")
    }

    async fn end_game_with_message(&self, http: &Http, session: &mut GameSession, original_session: &Arc<Mutex<GameSession>>, message: EditMessage) {
        session.player.1.delete_response(http).await.unwrap();
        session.player2.1.delete_response(http).await.unwrap();

        if let Some(val) = &mut session.player2.3 {
            val.edit(http, message.clone()).await.unwrap();
        }

        session.player.3.edit(http, message).await.unwrap();
        
        let mut games = self.sessions.lock().await;
        let pos = games.iter().position(|val| Arc::ptr_eq(val, original_session));
        games.swap_remove(pos.unwrap());
    }
}

fn draw_new_game_canvas() -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let mut canvas = ImageBuffer::new(300, 300);

    // Background
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(0, 0).of_size(300, 300),
        BACKGROUND,
    );

    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(98, 0).of_size(4, 300),
        GRAY,
    );

    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(198, 0).of_size(4, 300),
        GRAY,
    );

    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(0, 98).of_size(300, 4),
        GRAY,
    );

    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(0, 198).of_size(300, 4),
        GRAY,
    );

    canvas
}

async fn show_wait_and_common_message(
    http: &Http,
    interaction: &CommandInteraction,
    canvas: &ImageBuffer<Rgb<u8>, Vec<u8>>,
    player_name: &str,
    player2_name: &str,
    common_message: &mut Message,
    common_message2: Option<&mut Message>,
) {
    let embed = CreateEmbed::new()
        .title("Game in process")
        .description("Waiting for your turn.")
        .thumbnail("attachment://thumbnail.png");

    let action_row = generate_disabled_action_row();
    let attachment = generate_attachment_rgb8(canvas, "canvas.png");

    interaction.edit_response(http, EditInteractionResponse::new()
        .add_embed(embed)
        .components(vec![action_row])
        .new_attachment(attachment.clone())
    ).await.unwrap();

    let edited_message = EditMessage::new()
        .embed(CreateEmbed::new()
            .title(format!(
                "Game between {} and {} in the progress!",
                player_name,
                player2_name,
            ))
            .description("You can play this game too by using the `/play` command.")
            .attachment("canvas.png")
        )
        .attachment(attachment);

    if let Some(val) = common_message2 {
        val.edit(http, edited_message.clone()).await.unwrap();
    }

    common_message.edit(http, edited_message).await.unwrap();
}

async fn show_game_message(
    http: &Http,
    interaction: &CommandInteraction,
    cursor_pos: usize,
    map: &[GameCell],
    canvas: &ImageBuffer<Rgb<u8>, Vec<u8>>,
) {
    let embed = CreateEmbed::new()
    .title("Your turn")
    .description("Press arrows buttons for moving selection square.");

    let action_row = if map[cursor_pos] != GameCell::None {
        generate_game_action_row(true, cursor_pos)
    }
    else {
        generate_game_action_row(false, cursor_pos)
    };

    let mut cloned = canvas.clone();

    draw_select_outline(&mut cloned, cursor_pos);

    interaction.edit_response(http, EditInteractionResponse::new()
        .embed(embed)
        .components(vec![action_row])
        .new_attachment(generate_attachment_rgb8(&cloned, "canvas.png"))
    )
    .await
    .unwrap();
}

async fn update_game_message(http: &Http, interaction: &ComponentInteraction, session: &GameSession) {
    let embed = CreateEmbed::new()
        .title("Your turn")
        .description("Press arrows buttons for moving selection square.");

    let action_row = if session.map[session.cursor_pos] != GameCell::None {
        generate_game_action_row(true, session.cursor_pos)
    }
    else {
        generate_game_action_row(false, session.cursor_pos)
    };

    let mut cloned = session.canvas.clone();

    draw_select_outline(&mut cloned, session.cursor_pos);

    interaction.edit_response(http, EditInteractionResponse::new()
        .embed(embed)
        .components(vec![action_row])
        .new_attachment(generate_attachment_rgb8(&cloned, "canvas.png"))
    )
    .await
    .unwrap();
}

fn draw_select_outline(canvas: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, cell: usize) {
    match cell {
        0 => {
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 + 98, CELLS[cell].1).of_size(4, 102), 
                RED,
            );
    
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0, CELLS[cell].1 + 98).of_size(98, 4), 
                RED,
            );
        }

        1 => {
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 + 98, CELLS[cell].1).of_size(4, 102), 
                RED,
            );
    
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 + 98).of_size(100, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1).of_size(4, 98), 
                RED,
            );
        }

        2 => {
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 + 98).of_size(102, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1).of_size(4, 98), 
                RED,
            );
        }

        3 => {
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0, CELLS[cell].1 - 2).of_size(102, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 + 98, CELLS[cell].1 + 2).of_size(4, 100), 
                RED,
            );
    
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0, CELLS[cell].1 + 98).of_size(98, 4), 
                RED,
            );
        }

        4 => {
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 - 2).of_size(104, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 + 98, CELLS[cell].1 + 2).of_size(4, 100), 
                RED,
            );
    
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 + 98).of_size(100, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 + 2).of_size(4, 96), 
                RED,
            );
        }

        5 => {
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 - 2).of_size(102, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 + 98).of_size(102, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 + 2).of_size(4, 96), 
                RED,
            );
        }

        6 => {
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0, CELLS[cell].1 - 2).of_size(102, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 + 98, CELLS[cell].1 + 2).of_size(4, 98), 
                RED,
            );
        }

        7 => {
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 - 2).of_size(104, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 + 98, CELLS[cell].1 + 2).of_size(4, 98), 
                RED,
            );
    
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 + 2).of_size(4, 98), 
                RED,
            );
        }

        8 => {
            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 - 2).of_size(102, 4), 
                RED,
            );

            draw_filled_rect_mut(
                canvas,
                Rect::at(CELLS[cell].0 - 2, CELLS[cell].1 + 2).of_size(4, 98), 
                RED,
            );
        }

        _ => unreachable!(),
    }
}

fn generate_disabled_action_row() -> CreateActionRow {
    let left = CreateButton::new("left")
        .label("←")
        .style(ButtonStyle::Secondary)
        .disabled(true);
    
    let down = CreateButton::new("down")
        .label("↓")
        .style(ButtonStyle::Secondary)
        .disabled(true);

    let up = CreateButton::new("up")
        .label("↑")
        .style(ButtonStyle::Secondary)
        .disabled(true);

    let right = CreateButton::new("right")
        .label("→")
        .style(ButtonStyle::Secondary)
        .disabled(true);

    let send = CreateButton::new("send")
        .label("Send")
        .style(ButtonStyle::Primary)
        .disabled(true);

    let action_row = CreateActionRow::Buttons(vec![
        left,
        down,
        up,
        right,
        send,
    ]);

    action_row
}

fn generate_game_action_row(send_disabled: bool, cursor_position: usize) -> CreateActionRow {
    let mut left = CreateButton::new("left")
        .label("←")
        .style(ButtonStyle::Secondary);
    
    if [0, 3, 6].contains(&cursor_position) {
        left = left.disabled(true);
    }
    
    let mut down = CreateButton::new("down")
        .label("↓")
        .style(ButtonStyle::Secondary); 

    if cursor_position >= 6 {
        down = down.disabled(true);
    }

    let mut up = CreateButton::new("up")
        .label("↑")
        .style(ButtonStyle::Secondary);

    if cursor_position <= 2 {
        up = up.disabled(true);
    }

    let mut right = CreateButton::new("right")
        .label("→")
        .style(ButtonStyle::Secondary); 

    if [2, 5, 8].contains(&cursor_position) {
        right = right.disabled(true);
    }

    let send = CreateButton::new("send")
        .label("Send")
        .style(ButtonStyle::Primary)
        .disabled(send_disabled);

    let action_row = CreateActionRow::Buttons(vec![
        left,
        down,
        up,
        right,
        send,
    ]);

    action_row
}

fn generate_attachment(image: &[u8], width: u32, height: u32, name: &'static str, color_type: ColorType) -> CreateAttachment {
    let buffer = Vec::new();
    let cursor = Cursor::new(buffer);
    let mut buffered_writer = BufWriter::new(cursor);

    image::write_buffer_with_format(
        &mut buffered_writer,
        &image,
        width,
        height,
        color_type,
        ImageOutputFormat::Png,
    )
    .expect("failed to write in buffer");

    let buffer = buffered_writer.into_inner().unwrap().into_inner();

    CreateAttachment::bytes(buffer, name)
}

fn generate_attachment_rgb8(image: &ImageBuffer<Rgb<u8>, Vec<u8>>, name: &'static str) -> CreateAttachment {
    generate_attachment(image, image.width(), image.height(), name, ColorType::Rgb8)
}

fn fill_pixel(canvas: &mut ImageBuffer<Rgb<u8>, Vec<u8>>, scratch: &ImageBuffer<Rgba<u8>, Vec<u8>>, x: u32, y: u32) {
    let pixel = canvas.get_pixel(x, y).0;
    let pixel2 = scratch.get_pixel(x, y).0;

    let alpha = pixel2[3] as f32 / 255.0;
    let mut output = Rgb([0, 0, 0]);

    for i in 0..=2 {
        let pixel_f32 = pixel[i] as f32 / 255.0;
        let pixel2_f32 = pixel2[i] as f32 / 255.0;

        output.0[i] = ((pixel_f32 * (1.0 - alpha) + pixel2_f32 * alpha) * 255.0).clamp(0.0, 255.0) as u8;
    }

    canvas.draw_pixel(x, y, output);
}