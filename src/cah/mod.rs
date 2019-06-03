use csv::Reader;
use csv::Error as CsvError;
use rand::Rng;
use serde::{Serialize, Deserialize};
use serenity::Result as SerenityResult;
use serenity::client::Context;
use serenity::framework::standard::{Args, CommandError};
use serenity::model::channel::Message;
use serenity::model::id::UserId;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::io::Read;
use typemap::Key;
use std::fs::File;
use std::path::Path;
use std::io::Error as IoError;
use std::collections::VecDeque;
use serenity::model::id::ChannelId;

pub mod commands;

// One game of CAH. Not compatible with multiple instances. May change.
pub struct CahManager {
    hands: HashMap<UserId, Vec<WhiteCardId>>,
    wins: HashMap<UserId, Vec<BlackCardId>>,
    in_play: HashMap<UserId, Vec<WhiteCardId>>,
    state: State,
    black_deck: HashMap<BlackCardId, BlackCard>,
    white_deck: HashMap<WhiteCardId, WhiteCard>,
    black_deck_state: VecDeque<BlackCardId>,
    white_deck_state: VecDeque<WhiteCardId>,
    deck_names: Vec<String>,
    hand_size: u8,
    win_condition: u8,
    players: PlayerContainer,
    primary_channel: ChannelId,
    current_black_card: Option<BlackCardId>,
}

impl CahManager {
    // absolutely basic must-be-assigned fields, customize the rest later
    pub fn new(black_deck: Vec<BlackCard>, white_deck: Vec<WhiteCard>, deck_names: Vec<String>) -> Self {
        let black_deck = black_deck.into_iter().map(|x| (x.id, x)).collect::<HashMap<_, _>>();
        let white_deck = white_deck.into_iter().map(|x| (x.id, x)).collect::<HashMap<_, _>>();
        let black_deck_state = black_deck.keys().cloned().collect::<VecDeque<_>>();
        let white_deck_state = white_deck.keys().cloned().collect::<VecDeque<_>>();
        CahManager {
            hands: HashMap::new(),
            wins: HashMap::new(),
            state: State::Off,
            black_deck,
            white_deck,
            deck_names,
            black_deck_state,
            white_deck_state,
            hand_size: 10,
            in_play: HashMap::new(),
            win_condition: 4,
            players: PlayerContainer::new(),
            primary_channel: ChannelId::default(),
            current_black_card: None,
        }
    }
    pub fn is_running(&self) -> bool {
        self.state != State::Off
    }
    pub fn new_game(&mut self) -> bool {
        if self.state != State::Off {
            false
        } else {
            // init game
            self.state = State::Waiting;
            self.wins.clear();
            self.hands.clear();
            self.black_deck_state.clear();
            self.black_deck_state.extend(self.black_deck.keys().cloned());
            self.white_deck_state.clear();
            self.white_deck_state.extend(self.white_deck.keys().cloned());
            self.in_play.clear();
            true
        }
    }
    pub fn set_primary_channel(&mut self, id: ChannelId) {
        self.primary_channel = id;
    }
    pub fn get_primary_channel(&self) -> ChannelId {
        self.primary_channel
    }
    pub fn get_hand(&mut self, id: UserId) -> &mut Vec<WhiteCardId> {
        if self.hands.contains_key(&id) {
            // if they've got a hand, return it
            self.hands.get_mut(&id).unwrap()
        } else {
            // otherwise, let's make one. this is also the function to add new users to the game.
            let mut hand = Vec::with_capacity(self.hand_size as usize);
            for _ in 0..self.hand_size {
                hand.push(self.draw_white());
            }
            self.hands.insert(id, hand);
            self.players.add_player(id);
            self.hands.get_mut(&id).unwrap()
        }
    }
    pub fn get_white_card(&self, id: WhiteCardId) -> Option<&WhiteCard> {
        self.white_deck.get(&id)
    }
    pub fn get_black_card(&self, id: BlackCardId) -> Option<&BlackCard> {
        self.black_deck.get(&id)
    }
    pub fn get_deck_names(&self) -> &[String] {
        &self.deck_names
    }
    pub fn set_decks(&mut self, black_deck: Vec<BlackCard>, white_deck: Vec<WhiteCard>, deck_names: Vec<String>) {
        self.white_deck_state.clear();
        {
            let to_check = self.hands.values().flat_map(|x| x.iter());
            for card in white_deck.iter().map(|x| x.id) {
                // don't include any white cards in hands
                if !self.hands.values().any(|x| x.iter().any(|y| *y == card)) {
                    self.white_deck_state.push_front(card);
                }
            }
        }
        self.black_deck_state.clear();
        {
            let to_check = self.wins.values().flat_map(|x| x.iter());
            for card in black_deck.iter().map(|x| x.id) {
                if !self.wins.values().any(|x| x.iter().any(|y| *y == card)) {
                    self.black_deck_state.push_front(card);
                }
            }
        }
        let mut rng = rand::thread_rng();
        rng.shuffle_deque(&mut self.white_deck_state);
        rng.shuffle_deque(&mut self.black_deck_state);
        let black_deck_map = black_deck.into_iter().map(|x| (x.id, x)).collect::<HashMap<_, _>>();
        let white_deck_map = white_deck.into_iter().map(|x| (x.id, x)).collect::<HashMap<_, _>>();
        // explicitly not checking user hands because if they're replaced without the user checking their hands, they'll unintentionally play cards
        self.white_deck = white_deck_map;
        self.black_deck = black_deck_map;
    }
    pub fn get_hand_size(&self) -> u8 {
        self.hand_size
    }
    pub fn set_hand_size(&mut self, hand_size: u8) {
        self.hand_size = hand_size;
    }
    // mechanism for getting nonexistent cards and updating the hand simultaneously
    pub fn resolve_white_card(&mut self, id: WhiteCardId, user: UserId) -> &WhiteCard {
        if self.get_white_card(id).is_none() {
            let card = self.draw_white();
            //goddamnit intellij
            #[allow(unused_mut)]
            let mut hand = self.get_hand(user);
            let idx = hand.iter().position(|x| *x == id);
            if let Some(idx) = idx {
                hand[idx] = card;
            }
            self.get_white_card(card).unwrap()
        } else {
            self.get_white_card(id).unwrap()
        }
    }
    pub fn draw_white(&mut self) -> WhiteCardId {
        self.white_deck_state.pop_front().unwrap_or(WhiteCardId::default())
    }
    pub fn draw_black(&mut self) -> &BlackCard {
        if self.black_deck_state.len() == 0 {
            let mut black_deck_state = self.black_deck.keys().cloned().collect::<Vec<_>>();
            rand::thread_rng().shuffle(&mut black_deck_state);
            self.black_deck_state = black_deck_state.into();
        }
        let id = self.black_deck_state.pop_front().unwrap();
        self.get_black_card(id).unwrap()

    }
    pub fn get_players(&self) -> &PlayerContainer {
        &self.players
    }
    pub fn get_players_mut(&mut self) -> &mut PlayerContainer {
        &mut self.players
    }
    pub fn get_state(&self) -> State {
        self.state
    }
    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }
    pub fn draw_into_hand(&mut self, user: UserId, amount: usize) {
        for _ in 0..amount {
            let draw = self.draw_white();
            self.get_hand(user).push(draw);
        }
    }
    pub fn get_current_black_card(&self) -> Option<BlackCardId> {
        self.current_black_card
    }
    pub fn get_cards_in_play(&self, id: UserId) -> Option<&Vec<WhiteCardId>> {
        self.in_play.get(&id)
    }
    pub fn get_cards_in_play_mut(&mut self, id: UserId) -> &mut Vec<WhiteCardId> {
        if self.in_play.contains_key(&id) {
            self.in_play.get_mut(&id).unwrap()
        } else {
            let vec = Vec::with_capacity(5);
            self.in_play.insert(id, vec);
            self.in_play.get_mut(&id).unwrap()
        }
    }
    pub fn get_all_cards_in_play(&self) -> impl Iterator<Item=(UserId, &Vec<WhiteCardId>)> {
        self.in_play.iter().map(|(x, y)| (*x, y))
    }
    pub fn get_wins_mut(&mut self, id: UserId) -> &mut Vec<BlackCardId> {
        if self.wins.contains_key(&id) {
            self.wins.get_mut(&id).unwrap()
        } else {
            let vec = Vec::with_capacity(8);
            self.wins.insert(id, vec);
            self.wins.get_mut(&id).unwrap()
        }
    }
    pub fn clear_cards_in_play(&mut self) {
        for vec in self.in_play.values_mut() {
            vec.clear()
        }
    }
    pub fn get_win_condition(&self) -> u8 {
        self.win_condition
    }
}

impl Key for CahManager {
    type Value = CahManager;
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct WhiteCard {
    pub message: String,
    pub id: WhiteCardId,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct BlackCard {
    pub message: String,
    pub draw: u8,
    pub play: u8,
    pub id: BlackCardId,
}

pub fn parse_white_cards<R>(read: R) -> Result<Vec<WhiteCard>, CsvError> where R: Read {
    let mut reader = Reader::from_reader(read);
    let mut vec = Vec::new();
    for record in reader.deserialize() {
        vec.push(record?)
    }
    Ok(vec)
}

pub fn parse_black_cards<R>(read: R) -> Result<Vec<BlackCard>, CsvError> where R: Read {
    let mut reader = Reader::from_reader(read);
    let mut vec = Vec::new();
    for record in reader.deserialize() {
        vec.push(record?)
    }
    Ok(vec)
}

#[derive(Copy, Clone, Eq, Hash, PartialEq, Debug)]
pub enum State {
    Off,
    Playing,
    Reading,
    Waiting,
}

pub fn load_deck(name: &str) -> Result<(Vec<BlackCard>, Vec<WhiteCard>), IoError> {
    //todo cardcast
    let root = Path::new("decks");
    let white_file = File::open(root.join(&name).join("white.csv"))?;
    let white = parse_white_cards(white_file)?;
    let black_file = File::open(root.join(&name).join("black.csv"))?;
    let black = parse_black_cards(black_file)?;
    Ok((black, white))
}

pub struct PlayerContainer {
    ids: Vec<UserId>,
    index: usize,
}

impl PlayerContainer {
    pub fn new() -> Self {
        PlayerContainer {
            ids: Vec::new(),
            index: 0,
        }
    }
    pub fn add_player(&mut self, id: UserId) {
        self.ids.push(id);
    }
    pub fn remove_player(&mut self, id: UserId) -> bool {
        let idx = self.ids.iter().position(|x| *x == id);
        if let Some(idx) = idx {
            if self.index < idx {
                self.ids.remove(idx);
                if self.index < self.ids.len() {
                    self.index = 0;
                }
                true
            } else if self.index > idx {
                if self.index == 0 {
                    self.index = self.ids.len() - 1;
                } else {
                    self.index -= 1;
                }
                self.ids.remove(idx);
                true
            } else {
                false
            }
        } else {
            false
        }
    }
    pub fn remove_player_force(&mut self, id: UserId) -> bool {
        let idx = self.ids.iter().position(|x| *x == id);
        if let Some(idx) = idx {
            self.ids.remove(idx);
            if self.index >= idx {
                if self.index == 0 {
                    self.index = self.ids.len();
                } else {
                    self.index -= 1;
                }
            } else {
                if self.index == self.ids.len() {
                    self.index = 0;
                }
            }
            true
        } else {
            false
        }
    }
    pub fn next_player(&mut self) -> UserId {
        self.index += 1;
        if self.index == self.ids.len() {
            self.index = 0;
        }
        *self.ids.get(self.index).unwrap_or(&UserId(0))
    }
    pub fn current(&self) -> UserId {
        *self.ids.get(self.index).unwrap_or(&UserId(0))
    }
    pub fn contains(&self, id: UserId) -> bool {
        self.ids.contains(&id)
    }
    pub fn all(&self) -> &[UserId] {
        &self.ids
    }
}

trait RngExt {
    fn shuffle_deque<T>(&mut self, vec: &mut VecDeque<T>);
}

impl<R> RngExt for R where R: Rng {
    fn shuffle_deque<T>(&mut self, values: &mut VecDeque<T>) {
        let mut i = values.len();
        while i >= 2 {
            // invariant: elements with index >= i have been locked in place.
            i -= 1;
            // lock element i in place.
            values.swap(i, self.gen_range(0, i + 1));
        }
    }
}

pub fn format_card(card: &BlackCard, fills: &[&WhiteCard]) -> String {
    let mut pattern = card.message.clone();
    for fill in fills {
        if let Some(offset) = pattern.find("___") {
            pattern.replace_range(offset..offset + 3, &fill.message)
        } else {
            break;
        }
    }
    pattern
}

#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, Copy, Clone, Default)]
pub struct BlackCardId(u64);
#[derive(Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, Copy, Clone, Default)]
pub struct WhiteCardId(u64);
