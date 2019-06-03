use serenity::client::Context;
use serenity::framework::standard::Args;
use super::super::{CommandResult, get_name, get_name_other};
use serenity::framework::standard::CommandError;
use serenity::model::channel::Message;
use serenity::model::guild::Member;
use super::{CahManager, State, format_card};
use smallvec::SmallVec;
use std::fmt::Write;
use super::WhiteCardId;

pub fn new_game(c: &mut Context, m: &Message, _a: Args) -> CommandResult {
    let mut data = c.data.lock();
    let manager = data.get_mut::<CahManager>().ok_or_else(|| CommandError("Couldn't load the CahManager".to_string()))?;
    if !manager.new_game() {
        return Err(CommandError("A game is already running!".to_string()));
    }
    m.channel_id.say("Started CAH game.")?;
    manager.set_primary_channel(m.channel_id);
    c.set_game("Cards Against Humanity");
    Ok(())
}

pub fn my_cards(c: &mut Context, m: &Message, _a: Args) -> CommandResult {
    let mut data = c.data.lock();
    let manager = data.get_mut::<CahManager>().ok_or_else(|| CommandError("Couldn't load the CahManager".to_string()))?;
    if !manager.is_running() {
        return Err(CommandError("A game is not running!".to_string()));
    }
    let dm = m.author.create_dm_channel()?;
    if manager.get_players().contains(m.author.id) {
        let message = manager.get_hand(m.author.id)
            .to_vec()
            .iter()
            .enumerate()
            .map(|(idx, card)| idx.to_string() + ": " + &manager.resolve_white_card(*card, m.author.id).message + "\n")
            .collect::<String>();
        dm.say("Your cards: \n\n".to_string() + &message[..message.len() - 1])?;
    } else {
        dm.say("You are not in the game. Would you like to join? (Type `cah join` to join)".to_string())?;
    }
    Ok(())
}

pub fn join_game(c: &mut Context, m: &Message, a: Args) -> CommandResult {
    let mut data = c.data.lock();
    let manager = data.get_mut::<CahManager>().ok_or_else(|| CommandError("Couldn't load the CahManager".to_string()))?;
    if !manager.is_running() {
        return Err(CommandError("A game is not running!".to_string()));
    }
    let dm = m.author.create_dm_channel()?;
    if !manager.get_players().contains(m.author.id) {
        let message = manager.get_hand(m.author.id)
            .to_vec()
            .iter()
            .enumerate()
            .map(|(idx, card)| idx.to_string() + ": " + &manager.resolve_white_card(*card, m.author.id).message + "\n")
            .collect::<String>();
        dm.say("Your cards: \n\n".to_string() + &message[..message.len() - 1])?;
    } else {
        dm.say("You are already in the game.".to_string())?;
    }
    Ok(())
}

pub fn set_decks(c: &mut Context, m: &Message, mut a: Args) -> CommandResult {
    let mut data = c.data.lock();
    let manager = data.get_mut::<CahManager>().ok_or_else(|| CommandError("Couldn't load the CahManager".to_string()))?;
    let mut force = false;
    let mut no_base = false;
    loop {
        match a.current().ok_or_else(|| CommandError("Not enough arguments".to_string()))? {
            "+force" => force = true,
            "+no-base" => no_base = true,
            _ => break,
        }
        a.next();
    }
    if !force && manager.is_running() {
        return Err(CommandError("Game is running (add 'override' to disable".to_string()));
    }
    if manager.get_state() == State::Playing || manager.get_state() == State::Reading {
        return Err(CommandError("You can't change the decks while a black card is in play.".to_string()));
    }
    let mut decks = a.iter().map(|x| x.unwrap()).collect::<Vec<String>>();
    let base = "base".to_string();
    if !no_base && !decks.contains(&base) {
        decks.push(base)
    }
    let mut black_deck = Vec::new();
    let mut white_deck = Vec::new();
    for deck in decks.iter() {
        let (mut black, mut white) = super::load_deck(deck)?;
        black_deck.append(&mut black);
        white_deck.append(&mut white);
    }
    manager.get_primary_channel().say(format!("The decks have changed to: {:?}", decks))?;
    manager.set_decks(black_deck, white_deck, decks);
    Ok(())
}

pub fn get_decks(c: &mut Context, m: &Message, _a: Args) -> CommandResult {
    let data = c.data.lock();
    let manager = data.get::<CahManager>().ok_or_else(|| CommandError("Couldn't load the CahManager".to_string()))?;
    let mut string = "Loaded decks:\n\n".to_string();
    for deck in manager.get_deck_names() {
        string.push_str(deck);
        string.push_str("\n");
    }
    m.channel_id.say(string)?;
    Ok(())
}

pub fn draw_black_card(c: &mut Context, m: &Message, _a: Args) -> CommandResult {
    let mut data = c.data.lock();
    let manager = data.get_mut::<CahManager>().ok_or_else(|| CommandError("Couldn't load the CahManager".to_string()))?;
    if manager.get_players().current() != m.author.id {
        return Err(CommandError("It's not your turn".to_string()))
    }
    match manager.get_state() {
        State::Off => Err(CommandError("The game is not running".to_string())),
        State::Playing => Err(CommandError("A black card is already in play".to_string())),
        State::Reading => Err(CommandError("Wait for the winner to be announced".to_string())),
        State::Waiting => {
            let (draw, id) = {
                let card = manager.draw_black();
                m.channel_id.say(card.message.clone())?;
                if card.draw > 0 || card.play > 1 {
                    m.channel_id.say(format!("(Draw {}, play {})", card.draw, card.play))?;
                }
                (card.draw, card.id)
            };
            manager.current_black_card = Some(id);
            // stupid borrow checker
            let players = manager.get_players().all().iter().cloned().collect::<SmallVec<[_; 20]>>();
            for player in players {
                for _ in 0..draw {
                    let card = manager.draw_white();
                    manager.get_hand(player).push(card)
                }
                manager.set_state(State::Playing);
            }
            Ok(())
        }
    }
}

pub fn play_white_card(c: &mut Context, m: &Message, mut a: Args) -> CommandResult {
    let indices = a.iter::<usize>().collect::<Result<SmallVec<[_; 5]>, _>>()?;
    let mut data = c.data.lock();
    let manager = data.get_mut::<CahManager>().ok_or_else(|| CommandError("Couldn't load the CahManager".to_string()))?;
    match manager.get_state() {
        State::Off => Err(CommandError("The game is not running".to_string())),
        State::Reading => Err(CommandError("Wait for the next round".to_string())),
        State::Waiting => Err(CommandError("Wait for a black card to be chosen".to_string())),
        State::Playing => {
            if m.author.id == manager.get_players().current() {
                return Err(CommandError("You can't play a card - you're the Card Czar this round.".to_string()));
            }
            let hand = manager.get_hand(m.author.id).iter().cloned().collect::<SmallVec<[_; 20]>>();
            let card = manager.get_current_black_card().ok_or_else(|| CommandError("Internal error: Game is in State::Playing but there is no current black card".to_string()))?;
            {
                let card = manager.get_black_card(card).ok_or_else(|| CommandError("Internal error: Current black card is not a defined card".to_string()))?;
                if (card.play as usize) != indices.len() {
                    return Err(CommandError(format!("Wrong number of cards played (expected {}, got {})", card.play, indices.len())))
                }
                if let Some(x) = indices.iter().cloned().find(|x| hand.len() <= *x) {
                    return Err(CommandError(format!("{} is not a card in your hand.", x)))
                }
                let invalid = indices.iter().filter(|x| manager.get_white_card(hand[**x]).is_none()).collect::<SmallVec<[_; 5]>>();
                if invalid.len() != 0 {
                    let mut string = String::with_capacity(95);
                    string.push_str("Cards ");
                    for n in &invalid[..invalid.len() - 1] {
                        write!(&mut string, "{}, ", n)?;
                    }
                    write!(&mut string, "and {} are invalid (has the deck been reset?) Use `cah cards` to refresh.", invalid[invalid.len() - 1])?;
                    return Err(CommandError(string))
                }
            }
            {
                let cards = manager.get_cards_in_play_mut(m.author.id);
                cards.clear();
                for idx in indices.iter() {
                    cards.push(hand[*idx])
                }
            }
            manager.get_primary_channel().say(format!("{} has played.", get_name(m)))?;
            let czar = manager.get_players().current();
            let mut done = true;
            let not_czar = manager.get_players().all().iter().cloned().filter(|x| *x != czar).collect::<SmallVec<[_; 20]>>();
            for id in not_czar.iter().cloned() {
                if manager.get_cards_in_play(id).map(|x| x.is_empty()).unwrap_or(true) {
                    done = false;
                }
            }
            if done {
                manager.get_primary_channel().say(format!("Everyone has played. Now it's time for <@{}> to pick.", czar))?;
                let mut string = String::new();
                {
                    let card = manager.get_black_card(card).unwrap();
                    let mut values = manager.get_all_cards_in_play().map(|(id, x)| (id, x.iter().cloned().collect::<SmallVec<[WhiteCardId; 5]>>())).collect::<SmallVec<[_; 20]>>();
                    values.sort_by(|x, y| x.1.cmp(&y.1));
                    for (idx, cards) in values {
                        let selection = cards.iter().map(|x| manager.get_white_card(*x).unwrap()).collect::<SmallVec<[_; 5]>>();
                        write!(&mut string, "{}: {}\n", idx, &format_card(card, &selection))?;
                    }
                }
                manager.get_primary_channel().say(string)?;
                manager.set_state(State::Reading);
            }
            Ok(())
        },
    }
}

pub fn pick_winner(c: &mut Context, m: &Message, mut a: Args) -> CommandResult {
    let mut data = c.data.lock();
    let manager = data.get_mut::<CahManager>().ok_or_else(|| CommandError("Couldn't load the CahManager".to_string()))?;
    match manager.get_state() {
        State::Playing => Err(CommandError("Wait until everyone has played".to_string())),
        State::Waiting => Err(CommandError("The winner has already been picked".to_string())),
        State::Off => Err(CommandError("The game is not running".to_string())),
        State::Reading => {
            if m.author.id != manager.get_players().current() {
                return Err(CommandError("You're not the Card Czar.".to_string()));
            }
            let idx = a.single::<u8>()?;
            let mut values = manager.get_all_cards_in_play().map(|(id, x)| (id, x.iter().cloned().collect::<SmallVec<[WhiteCardId; 5]>>())).collect::<SmallVec<[_; 20]>>();
            values.sort_by(|x, y| x.1.cmp(&y.1));
            let (id, selection) = &values[idx as usize];
            let id = *id;
            let black_card = manager.get_current_black_card().ok_or_else(|| CommandError("Internal error: State::Reading but no active black card".to_string()))?;
            {
                let black_card = manager.get_black_card(black_card).ok_or_else(|| CommandError("Internal error: Active black card is invalid".to_string()))?;
                manager.get_primary_channel().say(format!("{} has chosen {}'s answer ({})", get_name(m), get_name_other(m, id), &format_card(black_card, &selection.iter().cloned().map(|x| manager.get_white_card(x).unwrap()).collect::<SmallVec<[_; 5]>>())))?;
            }
            let won = {
                let win_condition = manager.get_win_condition();
                let wins = manager.get_wins_mut(m.author.id);
                wins.push(black_card);
                wins.len() as u8 == win_condition
            };
            if won {
                manager.get_primary_channel().say(format!("{} has won the game! 🎉", get_name(m)))?;
                manager.set_state(State::Off);
                c.reset_presence();
            } else {
                manager.clear_cards_in_play();
                manager.set_state(State::Waiting);
                let next = manager.get_players_mut().next_player();
                manager.get_primary_channel().say(format!("It's <@{}>'s turn.", next.0))?;
            }
            Ok(())
        }
    }
}
