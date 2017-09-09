use std::collections::HashMap;

use std::io::{Result as IoResult};
use std::fs::{OpenOptions, File};
use std::path::Path;

use super::format_amount;

/// Stores the balance of the users of a room, in centimes
#[derive(Serialize, Deserialize)]
pub struct RoomTab {
        pub users: HashMap<String, i32>
}

impl RoomTab {
    pub fn new() -> RoomTab {
        RoomTab {
            users: HashMap::new()
        }
    }

    /// Shift the mean to ensure total balance in the room
    /// is near 0
    ///
    /// (there can be rounding issues depending on the number of
    /// people in the room)
    pub fn rebalance(&mut self) {
        let sum: i32 = self.users.values().sum();
        let mean = sum / self.users.len() as i32;
        for v in self.users.values_mut() {
            *v -= mean;
        }
    }

    pub fn format_balance(&self) -> String {
        let mut s: String = "Current room balance:".into();
        for (user, balance) in self.users.iter() {
            s.push_str(&format!("\n - {}: {}", user, format_amount(*balance)));
        }
        s
    }
}

#[derive(Serialize, Deserialize)]
pub struct TabStore {
    pub rooms: HashMap<String, RoomTab>
}

impl TabStore {
    pub fn new() -> TabStore {
        TabStore {
            rooms: HashMap::new()
        }
    }

    pub fn pay(&mut self, amount: i32, room: String, user: String) {
        let stored = self.rooms
                         .entry(room).or_insert_with(|| RoomTab::new())
                         .users.entry(user).or_insert(0);
        *stored += amount;
    }

    pub fn balance(&self, room: &str) -> String {
        self.rooms.get(room).map(|r| r.format_balance())
            .unwrap_or_else(|| "The tab of this room is currently empty.".into())
    }

    pub fn rebalance(&mut self, room: &str) {
        if let Some(tab) = self.rooms.get_mut(room) {
            tab.rebalance();
        }
    }

    pub fn save_to<P: AsRef<Path>>(&self, path: P) -> IoResult<()> {
        let file = OpenOptions::new().write(true)
                                     .create(true)
                                     .truncate(true)
                                     .open(path)?;
        ::serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    pub fn load_from<P: AsRef<Path>>(path: P) -> IoResult<Self> {
        let file = File::open(path)?;
        let me = ::serde_json::from_reader(file)?;
        Ok(me)
    }
}

