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

use futures::{Future, Stream};

use tokio_core::reactor::Core;

use gm::{MatrixClient, MatrixFuture};
use gm::errors::{MatrixError, MatrixErrorKind};
use gm::room::{RoomClient, RoomExt};
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
    let config = match Config::from_args() {
        Ok(cfg) => cfg,
        Err(()) => return
    };

    // init the store
    let mut store = TabStore::new();

    // get connexion password
    println!("Type password for the bot (characters won't show up as you type them)");
    let password = match prompt_password_stdout("password:") {
        Ok(p) => p,
        Err(err) => {
            println!("FATAL: failed to get password: {}", err);
            return;
        }
    };

    let namespace_type = format!("{}.tab", config.namespace);

    // setup matrix connexion
    let mut core = Core::new().unwrap();
    let hdl = core.handle();
    let mut mx = core.run(MatrixClient::login(&config.username, &password, &config.server, &hdl))
        .unwrap();
    println!("[+] Connected to {} as {}", config.server, config.username);
    let mut stream = mx.get_sync_stream();

    // initial sync
    let loaded_tabs = {
        let fut = stream.by_ref().take(1).map(|sync| {
            let mut futs: Vec<MatrixFuture<Option<(String, RoomTab)>>> = Vec::new();

            // join invite rooms
            for (room, _) in sync.rooms.invite {
                println!("[+] Joining {}", room.id);
                futs.push(Box::new(mx.join(&room.id).map(|_| None)));
            }

            // load previously joined room
            for (room, _) in sync.rooms.join {
                let mut cli = room.cli(&mut mx);
                let rid = room.id.to_string();
                let fut = cli.get_state(&namespace_type, None)
                            .map(|tab| Some((rid, tab)))
                            .or_else(|_| futures::future::ok(None));
                futs.push(Box::new(fut));
            }

            futures::future::join_all(futs.into_iter())
        }).into_future();
        let loaded_tabs_fut = match core.run(fut) {
            Ok((Some(tabs), _)) => tabs,
            Ok((None, _)) => panic!("Connexion lost before first sync ?!"),
            Err((e, _)) => panic!("Initial sync failed: {:?}", e)
        };
        core.run(loaded_tabs_fut).unwrap()
    };
    for (rid, tab) in loaded_tabs.into_iter().flat_map(|o| o) {
        store.restore(rid, tab);
    }

    // main loop
    let fut = stream.by_ref().for_each(|sync| {
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
                        handle_message(&mut cli, sender, body, &namespace_type, &mut store, &mut futs);
                    }
                    _ => {}
                }
            }
        }
        futures::future::join_all(futs.into_iter()).map(|_| ())
    });
    core.run(fut).unwrap();
}

fn set_state(room: &mut RoomClient, evt_type: &str, tab: &RoomTab) -> MatrixFuture<()> {
    let id = room.room.id.to_string();
    let fut = room.set_state(evt_type, None, tab)
        .map(|_| ())
        .or_else(move |e| {
            match e {
                MatrixError(MatrixErrorKind::BadRequest(ref repl),_) if repl.errcode == "M_FORBIDDEN" => {
                    // we do not have permition to modify the room state
                    println!("[!] Unable to store state in room {}", id);
                    println!("[!] Reason: {}", repl.error);
                    futures::future::ok(())
                }
                e => futures::future::err(e)
            }
        });
    Box::new(fut)
}

fn handle_message(
    room: &mut RoomClient,
    sender: String,
    body: String,
    evt_type: &str,
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
            futs.push(set_state(room, evt_type, store.get(&rid).unwrap()));
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
            if let Some(tab) = store.get(&rid) {
                futs.push(set_state(room, evt_type, tab));
            };
            // send it
            futs.push(Box::new(room.send_simple(store.balance(&rid)).map(|_| ())) as Box<_>);
        }
        Some("!paidto") => if let (Some(txt), Some(amount)) =
            (splits.next(), splits.next().and_then(parse_amount))
        {
            let msg = match store.payto(amount, rid.clone(), sender.clone(), &txt) {
                Ok(other) => {
                    let rest = splits.collect::<Vec<_>>();
                    futs.push(set_state(room, evt_type, store.get(&rid).unwrap()));
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
