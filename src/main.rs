extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

extern crate futures;
extern crate glitch_in_the_matrix as gm;
extern crate rpassword;
extern crate tokio_core;
extern crate clap;

mod tabs;
mod utils;
mod config;

use std::io;

use futures::{Future, Stream};

use tokio_core::reactor::Core;

use gm::{MatrixClient, MatrixFuture};
use gm::room::RoomClient;
use gm::types::messages::Message;
use gm::types::content::Content;
use gm::types::events::{MetaFull, MetaMinimal, Event};

use rpassword::prompt_password_stdout;

use tabs::*;
use utils::*;
use config::*;

static PAID_USAGE: &'static str = r#"Usage: !paid <amount> <Any description you like>
<amount> must be positive, without units, using `.` as cent separator"#;

static PAIDTO_USAGE: &'static str = r#"Usage: !paidto <username> <amount> <Any description you like>
<amount> must be positive, without units, using `.` as cent separator"#;

fn main() {
    // Read args
    let config = Config::from_args();

    // init the store
    let mut store = match TabStore::load_from(&config.store_file) {
        Ok(s) => {
            println!("[+] Loaded tab store from `{}`.", config.store_file);
            s
        }
        Err(err) => match err.kind() {
            io::ErrorKind::NotFound => {
                println!(
                    "[+] File `{}` does not exist, initializing an empty tab store.",
                    config.store_file
                );
                TabStore::new()
            }
            _ => {
                println!("FATAL: cannot open tab store file `{}`: {}", config.store_file, err);
                return;
            }
        },
    };

    // get connexion password
    println!("Type password for the bot (characters won't show up as you type them)");
    let password = match prompt_password_stdout("password:") {
        Ok(p) => p,
        Err(err) => {
            println!("FATAL: failed to get password: {}", err);
            return;
        }
    };

    // setup matrix connexion
    let mut core = Core::new().unwrap();
    let hdl = core.handle();
    let mut mx = core.run(MatrixClient::login(&config.username, &password, &config.server, &hdl))
        .unwrap();
    println!("[+] Connected to {} as {}", config.server, config.username);
    let stream = mx.get_sync_stream();

    // main loop
    let fut = stream.skip(1).for_each(|sync| {
        let mut futs: Vec<MatrixFuture<()>> = Vec::new();

        // join invite rooms
        for (room, _) in sync.rooms.invite {
            println!("[+] Joining {}", room.id);
            futs.push(Box::new(mx.join(&room.id).map(|_| ())));
        }

        // handle messages in joined rooms
        for (room, events) in sync.rooms.join {
            for event in events.timeline.events {
                // we only check regular messages
                match event {
                    Event::Full(
                        MetaFull { sender, .. },
                        Content::RoomMessage(Message::Text { body, .. })
                    ) |
                    Event::Minimal(
                        MetaMinimal { sender: Some(sender), .. },
                        Content::RoomMessage(Message::Text { body, .. })
                    ) => {
                        let mut cli = room.cli(&mut mx);
                        handle_message(&mut cli, sender, body, &mut store, &mut futs);
                    }
                    _ => {}
                }
            }
        }
        if let Err(err) = store.save_to(&config.store_file) {
            println!(
                "ERROR: could not write tab store to `{}`: {}",
                config.store_file,
                err
            );
        }
        futures::future::join_all(futs.into_iter()).map(|_| ())
    });
    core.run(fut).unwrap();
}

fn handle_message(
    room: &mut RoomClient,
    sender: String,
    body: String,
    store: &mut TabStore,
    futs: &mut Vec<MatrixFuture<()>>,
) {
    let rid = (*room.room.id).to_owned();
    let mut splits = body.split_whitespace();
    match splits.next() {
        Some("!paid") => if let Some(amount) = splits.next().and_then(parse_amount) {
            store.pay(amount, rid.clone(), sender.clone());
            let rest = splits.collect::<Vec<_>>();
            let msg = format!(
                "{} paid {} for \"{}\"",
                sender,
                format_amount(amount),
                rest.join(" ")
            );
            futs.push(Box::new(room.send_simple(msg).map(|_| ())) as Box<_>);
        } else {
            futs.push(Box::new(room.send_simple(PAID_USAGE).map(|_| ()))
                as Box<_>);
        },
        Some("!balance") => {
            // send the balance
            futs.push(Box::new(room.send_simple(store.balance(&rid)).map(|_| ())) as Box<_>);
        }
        Some("!rebalance") => {
            // update the balance
            futs.push(Box::new(
                room.send_simple("Rebalancing accounts to 0 mean.")
                    .map(|_| ()),
            ) as Box<_>);
            store.rebalance(&rid);
            // send it
            futs.push(Box::new(room.send_simple(store.balance(&rid)).map(|_| ())) as Box<_>);
        }
        Some("!paidto") => if let (Some(txt), Some(amount)) =
            (splits.next(), splits.next().and_then(parse_amount))
        {
            let msg = match store.payto(amount, rid.clone(), sender.clone(), &txt) {
                Ok(other) => {
                    let rest = splits.collect::<Vec<_>>();
                    format!(
                        "{} paid {} to {} for \"{}\"",
                        sender,
                        format_amount(amount),
                        other,
                        rest.join(" ")
                    )
                }
                Err(SearchError::Ambiguous) => format!("Name \"{}\" is ambiguous.", txt),
                Err(SearchError::NotFound) => format!(
                    "Name \"{}\" is unknown.\n
                    Tip: they may need to issue a \"!paid 0\" command for me to know them.",
                    txt
                ),
            };
            futs.push(Box::new(room.send_simple(msg).map(|_| ())) as Box<_>);
        } else {
            futs.push(Box::new(room.send_simple(PAIDTO_USAGE).map(|_| ()))
                as Box<_>);
        },
        _ => {}
    }
}
