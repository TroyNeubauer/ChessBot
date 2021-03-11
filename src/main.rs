extern crate tokio;

use ::serde::{Deserialize, Serialize};
use serenity::{
    async_trait,
    framework::standard::{
        macros::{check, command, group, help, hook},
        Args, CommandGroup, CommandOptions, CommandResult, DispatchError, HelpOptions, Reason,
        StandardFramework,
    },
    http::Http,
    model::{
        channel::{Channel, Message},
        gateway::Ready,
    },
};

use uuid::Uuid;

use serenity::prelude::*;

use std::{collections::HashSet, env};

extern crate derive_new;

#[group]
#[commands(check, help)]
struct General;

struct Handler;

#[derive(Serialize, Deserialize, Debug)]
struct Book {
    name: String,
    author: String,
    uuid: Uuid,
    total_count: u32,
}

#[derive(Serialize, Deserialize, Debug)]
struct CheckoutInstance {
    start: chrono::DateTime<chrono::offset::Local>,
    end: chrono::DateTime<chrono::offset::Local>,
    book: Uuid,
    discord_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct LibraryDB {
    books: Vec<Book>,
    checkouts: Vec<CheckoutInstance>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[hook]
async fn before(_ctx: &Context, msg: &Message, command_name: &str) -> bool {
    println!(
        "Got command '{}' by user '{}'",
        command_name, msg.author.name
    );

    true
}

#[hook]
async fn after(_ctx: &Context, _msg: &Message, command_name: &str, command_result: CommandResult) {
    match command_result {
        Ok(()) => println!("Processed command '{}'", command_name),
        Err(why) => println!("Command '{}' returned error {:?}", command_name, why),
    }
}

#[hook]
async fn unknown_command(ctx: &Context, msg: &Message, unknown_command_name: &str) {
    println!("Could not find command named '{}'", unknown_command_name);
    let _ = msg
        .reply(
            ctx,
            format!(
                "Unknown command \"{}\". Try !help for a list of available commands",
                unknown_command_name
            ),
        )
        .await;
}

#[hook]
async fn normal_message(_ctx: &Context, msg: &Message) {
    println!("Message is not a command '{}'", msg.content);
}

const LIBRARY_DB_NAME: &str = "library-db.bin";

async fn load_library_db() -> Option<LibraryDB> {
    let task = tokio::fs::read(LIBRARY_DB_NAME).await;
    match task {
        Ok(data) => {
            let result: Result<LibraryDB, _> = bincode::deserialize(&data);

            //We want to panic on failure
            let db = result.unwrap();
            println!("Loaded library: {:?} from disk successfully", db);
            Some(db)
        }
        Err(err) => {
            println!("Failed to load library file: {:?}", err);
            None
        }
    }
}

async fn save_library_db(db: &LibraryDB) -> Result<(), Box<dyn std::error::Error>> {
    let data: Vec<u8> = bincode::serialize(db)?;
    tokio::fs::write(LIBRARY_DB_NAME, data).await?;

    println!("Saved library database successfully");
    Ok(())
}

async fn try_save_db(db: &LibraryDB) {
    match save_library_db(db).await {
        Ok(_) => {}
        Err(err) => {
            println!("An error occured while trying to save thi library database!");
            println!("{:?}", err);
            println!("Dumping database json to stdout:");
            let json = serde_json::to_string(&db).unwrap();
            println!("{}", json);
            let mut temp_file = std::env::temp_dir();
            temp_file.push("ERAU-discord-bot-library-backup.bin");

            println!("Also writing to temp file {:?}", temp_file.to_str());
            let _ = tokio::fs::write(temp_file, json).await;
        }
    }
}

#[tokio::main]
async fn main() {
    let prev_db = load_library_db().await;
    let mutex = std::sync::Mutex::new(match prev_db {
        Some(lib) => lib,
        None => LibraryDB::new(),
    });

    tokio::task::spawn(async {
        // Login with a bot token from the environment
        let token = env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN enviroment variable not set");

        let http = Http::new_with_token(&token);

        // We will fetch your bot's owners and id
        let (owners, bot_id) = match http.get_current_application_info().await {
            Ok(info) => {
                let mut owners = HashSet::new();
                if let Some(team) = info.team {
                    owners.insert(team.owner_user_id);
                } else {
                    owners.insert(info.owner.id);
                }
                match http.get_current_user().await {
                    Ok(bot_id) => (owners, bot_id.id),
                    Err(why) => panic!("Could not access the bot id: {:?}", why),
                }
            }
            Err(why) => panic!("Could not access application info: {:?}", why),
        };

        let framework = StandardFramework::new()
            .configure(|c| c.on_mention(Some(bot_id)).owners(owners).prefix("!"))
            .before(before)
            .after(after)
            .unrecognised_command(unknown_command)
            .normal_message(normal_message)
            .group(&GENERAL_GROUP);

        let mut client = Client::builder(token)
            .event_handler(Handler)
            .framework(framework)
            .await
            .expect("Error creating client");

        if let Err(why) = client.start().await {
            println!("An error occurred while running the client: {:?}", why);
        }
    });

    let _ = tokio::signal::ctrl_c().await;
    println!("Interupt recieved. Shutting down");
    try_save_db(&mutex.lock().unwrap()).await;
}

#[command]
async fn check(ctx: &Context, msg: &Message) -> CommandResult {
    msg.reply(ctx, "mate").await?;

    Ok(())
}

#[command]
async fn help(ctx: &Context, msg: &Message) -> CommandResult {
    msg.reply(ctx, "Help screen TODO").await?;
    println!("Content {}", msg.content);

    Ok(())
}

impl LibraryDB {
    fn new() -> LibraryDB {
        LibraryDB {
            books: Vec::new(),
            checkouts: Vec::new(),
        }
    }
}
