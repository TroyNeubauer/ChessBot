extern crate tokio;

use serenity::{
    async_trait,
    framework::standard::{
        help_commands,
        macros::{check, command, group, help, hook},
        Args, CommandGroup, CommandOptions, CommandResult, DispatchError, HelpOptions, Reason,
        StandardFramework,
    },
    http::Http,
    model::{
        channel::{Channel, Message},
        gateway::Ready,
        id::UserId,
        permissions::Permissions,
    },
};

use serenity::prelude::*;

use std::collections::HashSet;
use std::env;
use std::fmt::Write;

use signal_hook::iterator::Signals;

use lazy_static::lazy_static;

mod library;

extern crate derive_new;

#[group]
#[commands(check)]
struct General;

#[group]
// Sets a single prefix for this group.
// So one has to call commands in this group
// via `~library XXX` instead of just `~ XXX`.
#[prefix = "library"]
#[description = "Commands to query, checkout, or update information about books owned by this chess club"]
#[commands(list, checkout, return_command)]
struct Library;

// The framework provides two built-in help commands for you to use.
// But you can also make your own customized help command that forwards
// to the behaviour of either of them.
#[help]
// This replaces the information that a user can pass
// a command-name as argument to gain specific information about it.
#[individual_command_tip = "If you want more information about a specific command, just pass the command as argument."]
// Some arguments require a `{}` in order to replace it with contextual information.
// In this case our `{}` refers to a command's name.
#[command_not_found_text = "Could not find: `{}`."]
#[max_levenshtein_distance(2)]
// When you use sub-groups, Serenity will use the `indention_prefix` to indicate
// how deeply an item is indented.
// The default value is "-", it will be changed to "+".
#[indention_prefix = "+"]
// On another note, you can set up the help-menu-filter-behaviour.
// Here are all possible settings shown on all possible options.
// First case is if a user lacks permissions for a command, we can hide the command.
#[lacking_permissions = "Strike"]
// If the user is nothing but lacking a certain role, we just display it hence our variant is `Nothing`.
#[lacking_role = "Nothing"]
// The last `enum`-variant is `Strike`, which ~~strikes~~ a command.
#[wrong_channel = "Strike"]
// Serenity will automatically analyse and generate a hint/tip explaining the possible
// cases of ~~strikethrough-commands~~, but only if
// `strikethrough_commands_tip_in_{dm, guild}` aren't specified.
// If you pass in a value, it will be displayed instead.
async fn my_help(
    context: &Context,
    msg: &Message,
    args: Args,
    help_options: &'static HelpOptions,
    groups: &[&'static CommandGroup],
    owners: HashSet<UserId>,
) -> CommandResult {
    let _ = help_commands::with_embeds(context, msg, args, help_options, groups, owners).await;
    Ok(())
}
struct Handler;

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

async fn init() -> Result<(library::Database, Client), Box<dyn std::error::Error>> {
    let prev_db = library::Database::load().await;

    // Login with a bot token from the environment
    let token = env::var("DISCORD_TOKEN")?;

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
        .help(&MY_HELP)
        .group(&GENERAL_GROUP)
        .group(&LIBRARY_GROUP);

    let client = Client::builder(token)
        .event_handler(Handler)
        .framework(framework)
        .await?;

    //Assign the database if we make it this far because this is how we tell if if
    //initalization succeded
    let database = match prev_db {
        Some(lib) => lib,
        None => library::Database::new(),
    };
    Ok((database, client))
}

lazy_static! {
    static ref STATIC_DB: std::sync::Mutex<std::cell::RefCell<Option<library::Database>>> =
        std::sync::Mutex::new(std::cell::RefCell::new(None));
}

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let init_result = rt.block_on(init());

    match init_result {
        Ok((tmp_database, bad_client)) => {
            {
                let ref_cell = STATIC_DB.lock().unwrap();
                ref_cell.replace(Some(tmp_database));
            }
            //Leaking is ok because the program will exit when the future returns and there is no
            //other way to easily get 'static
            let client = Box::leak(Box::new(bad_client));
            let client_future = client.start();
            let client_join = rt.spawn(client_future);

            //client_join.abort();
            println!("Waiting on SIGINT or SIGTERM");
            let _ = Signals::new(&[signal_hook::SIGINT, signal_hook::SIGTERM])
                .unwrap()
                .wait();

            println!("Got signal. Stopping runtime");
            client_join.abort();

            rt.block_on(async {
                let mut ref_cell = STATIC_DB.lock().unwrap();
                let database: &library::Database = ref_cell.get_mut().as_ref().unwrap();
                library::Database::save(database).await.unwrap();
            });
        }
        Err(err) => {
            println!("Error {}", err);
        }
    }
}

#[command]
#[description = "Lists the books in the library and other information such as author and availability"]
async fn list(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    let mut responce = String::new();
    {
        let mut ref_cell = STATIC_DB.lock().unwrap();
        let database: &library::Database = ref_cell.get_mut().as_ref().unwrap();

        write!(
            responce,
            "The library contains {} book(s)",
            database.books.len()
        )?;
        for book in &database.books {
            write!(
                responce,
                "  *{}* by {} - {}",
                book.1.name,
                book.1.author,
                library::Database::encode_uuid(book.1.uuid)
            )?;
        }
    }

    msg.reply(ctx, responce).await?;

    Ok(())
}

#[command]
#[description = "Starts a checkout transaction for a book. Use this to checkout a book in the library"]
async fn checkout(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    msg.reply(ctx, "mate").await?;

    Ok(())
}

#[command("return")]
#[description = "Used to indicate that you have returned a book to an officer"]
async fn return_command(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    msg.reply(ctx, "mate").await?;

    Ok(())
}

#[command]
#[description = "Checks the status of the bot. Replies mate if the bot is online and operational"]
async fn check(ctx: &Context, msg: &Message) -> CommandResult {
    msg.reply(ctx, "mate").await?;

    Ok(())
}
