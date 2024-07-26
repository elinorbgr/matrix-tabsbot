mod tabs;
mod utils;
mod handlers;

use std::sync::Arc;

use anyhow::bail;

use handlers::on_room_message;
use rpassword::prompt_password;

use clap::Parser;

use matrix_sdk::{
    config::SyncSettings, deserialized_responses::SyncOrStrippedState, ruma::events::SyncStateEvent, Client
};

use tabs::{RoomTabContent, TabStore};
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
pub struct Config {
    /// URL of the homeserver to connect to
    #[arg(short, long)]
    pub server: String,
    /// Username of the bot
    #[arg(short, long)]
    pub username: String,
}

async fn login_and_run(config: Config, password: String) -> anyhow::Result<()> {
    let client = Client::builder().homeserver_url(config.server).build().await?;

    client.matrix_auth().login_username(config.username, &password).await?;

    // handler for autojoin
    client.add_event_handler(crate::handlers::on_stripped_state_member);

    // initial sync
    let sync_token = client.sync_once(SyncSettings::default()).await.unwrap().next_batch;

    let mut store = TabStore::new();

    // load the state from the known rooms
    for room in client.joined_rooms() {
        if let Some(tab_event) = room.get_state_event_static::<RoomTabContent>().await? {
            if let SyncOrStrippedState::Sync(SyncStateEvent::Original(room_tab)) = tab_event.deserialize()? {
                store.restore(room.room_id(), room_tab.content);
            }
        }
    }

    client.add_event_handler_context(Arc::new(Mutex::new(store)));
    client.add_event_handler(on_room_message);

    let settings = SyncSettings::default().token(sync_token);
    client.sync(settings).await?;

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // Read args
    let config = Config::parse();

    // get connexion password
    println!("Type password for the bot (characters won't show up as you type them)");
    let password = match prompt_password("password:") {
        Ok(p) => p,
        Err(err) => {
            bail!("FATAL: failed to get password: {}", err);
        }
    };

    login_and_run(config, password).await?;

    Ok(())
}