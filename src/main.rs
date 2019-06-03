#![allow(unused_variables, dead_code, unused_imports)]
#![cfg_attr(feature = "cargo-clippy", warn(clippy))]
#![feature(nll)]

extern crate serenity;
extern crate typemap;
extern crate csv;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate toml;
extern crate smallvec;
extern crate rand;

use std::mem;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use self::cah::CahManager;
use serenity::client::{Client, Context, EventHandler};
use serenity::framework::standard::{Args, CommandError, StandardFramework};
use serenity::model::channel::Message;
use serenity::model::id::UserId;

pub mod cah;

type CommandResult = Result<(), CommandError>;

fn main() {
    let mut config_file = File::open("romeo.toml").expect("Error opening config");
    let mut config_str = String::new();
    config_file.read_to_string(&mut config_str).expect("Error loading config");
    let mut config = toml::from_str::<Config>(&config_str).expect("Error parsing config");
    let mut client = Client::new(&config.token, Handler).expect("Error creating client");
    let framework = StandardFramework::new().configure(|c| c.on_mention(true).no_dm_prefix(true).prefix("."))
        .on("ping", ping)
        .group("cah", |g| g
            .command("cah new", |c| c.guild_only(true).exec(cah::commands::new_game))
            .command("cah join", |c| c.guild_only(true).exec(cah::commands::join_game))
            .command("cah cards", |c| c.dm_only(false /*todo*/).exec(cah::commands::my_cards))
            .command("cah draw", |c| c.guild_only(true).exec(cah::commands::draw_black_card))
            .command("cah play", |c| c.dm_only(false /*todo*/).exec(cah::commands::play_white_card))
            .on("cah decks", cah::commands::get_decks)
            .command("cah set-decks", |c| c.guild_only(true).exec(cah::commands::set_decks))
            .command("cah pick", |c| c.guild_only(true).exec(cah::commands::pick_winner)))
        .after(command_error_handler);
    client.with_framework(framework);
    let mut white_cards = Vec::new();
    let mut black_cards = Vec::new();
    for deck in config.cah.default_decks.iter() {
        let (mut black, mut white) = cah::load_deck(deck).expect(&format!("Error loading deck {}", deck));
        white_cards.append(&mut white);
        black_cards.append(&mut black);
    }
    {
        let mut data = client.data.lock();
        let mut default_decks = Vec::new();
        //MiCrO-oPtImIzAtIoNs ArE UsElEsS
        mem::swap(&mut default_decks, &mut config.cah.default_decks);
        let cah_manager = CahManager::new(black_cards, white_cards, default_decks);
        data.insert::<CahManager>(cah_manager);
    }
    client.start().expect("Error occurred starting client")
}

fn get_name(message: &Message) -> String {
    message.guild_id.and_then(|g| g.member(message.author.id).ok()).and_then(|m| m.nick.clone()).unwrap_or(message.author.name.clone())
}

fn get_name_other(message: &Message, id: UserId) -> String {
    message.guild_id.and_then(|g| g.member(id).ok()).and_then(|m| m.nick.clone()).unwrap_or_else(|| id.to_user().ok().map(|u| u.name.clone()).unwrap_or_else(|| format!("<@{}>", id.0)))
}

fn command_error_handler(_c: &mut Context, m: &Message, _name: &str, res: CommandResult) {
    if let Err(err) = res {
        m.channel_id.say(err.0).ok();
    }
}

struct Handler;
impl EventHandler for Handler {}

fn ping(_c: &mut Context, m: &Message, _a: Args) -> CommandResult {
    m.channel_id.say("Pong!")?;
    Ok(())
}

#[derive(Deserialize)]
struct Config {
    token: String,
    cah: CahSection,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CahSection {
    default_decks: Vec<String>,
}
