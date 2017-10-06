extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

extern crate futures;
extern crate glitch_in_the_matrix as gm;
extern crate rpassword;
extern crate tokio_core;

mod tabs;

use std::{env, io};

use futures::{Future, Stream};

use tokio_core::reactor::Core;

use gm::{MatrixClient, MatrixFuture};
use gm::types::messages::Message;
use gm::types::content::Content;
use gm::types::events::EventTypes;

use rpassword::prompt_password_stdout;

use tabs::*;


static PAID_USAGE: &'static str = r#"Usage: !paid <amount> <Any description you like>
<amount> must be positive, without units, using `.` as cent separator"#;

static PAIDTO_USAGE: &'static str = r#"Usage: !paidto <username> <amount> <Any description you like>
<amount> must be positive, without units, using `.` as cent separator"#;


fn parse_amount(txt: &str) -> Option<i32> {
    let mut splits = txt.split('.').map(|s| (s.len(), s.parse::<i32>()));
    match (splits.next(), splits.next(), splits.next()) {
        (Some((_, Ok(units))), Some((d, Ok(mut cents))), None) => {
            if d > 2 {
                return None;
            } else if d == 1 {
                cents *= 10;
            }
            if units < 0 {
                return None;
            }
            Some(units * 100 + cents)
        }
        (Some((_, Ok(units))), None, None) => if units >= 0 {
            Some(units * 100)
        } else {
            None
        },
        _ => None,
    }
}

fn format_amount(amount: i32) -> String {
    let (sign, amount) = if amount < 0 {
        ("-", -amount)
    } else {
        ("", amount)
    };
    let units = amount / 100;
    let cents = amount % 100;
    format!(
        "{}{}.{}{}",
        sign,
        units,
        if cents < 10 { "0" } else { "" },
        cents
    )
}

fn main() {
    // Read args
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        println!("Usage: matrix-tabs SERVER USERNAME STOREFILE");
        return;
    }
    let (server, username, storefile) = (&args[0], &args[1], &args[2]);

    // init the store
    let mut store = match TabStore::load_from(storefile) {
        Ok(s) => {
            println!("[+] Loaded tab store from `{}`.", storefile);
            s
        }
        Err(err) => match err.kind() {
            io::ErrorKind::NotFound => {
                println!(
                    "[+] File `{}` does not exist, initializing an empty tab store.",
                    storefile
                );
                TabStore::new()
            }
            _ => {
                println!("FATAL: cannot open tab store file `{}`: {}", storefile, err);
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
    let mut mx = core.run(MatrixClient::login(username, &password, server, &hdl))
        .unwrap();
    println!("[+] Connected to {} as {}", server, username);
    let stream = mx.get_sync_stream();

    // main loop
    let fut = stream.skip(1).for_each(|sync| {
        let mut futs: Vec<MatrixFuture<()>> = Vec::new();

        // join invite rooms
        for (rid, _) in sync.rooms.invite {
            println!("[+] Joining {}", rid);
            futs.push(Box::new(mx.join(&rid).map(|_| ())));
        }

        // handle messages in joined rooms
        for (rid, room) in sync.rooms.join {
            for event in room.timeline.events {
                // we only check regular messages
                if let EventTypes::Event(event) = event {
                    let sender = event.sender;
                    if let Content::Message(msg) = event.content {
                        if let Message::Text { body, .. } = msg {
                            // This is a regular text message, we may need to process it
                            handle_message(&mut mx, &rid, sender, body, &mut store, &mut futs);
                        }
                    }
                }
            }
        }
        if let Err(err) = store.save_to(storefile) {
            println!(
                "ERROR: could not write tab store to `{}`: {}",
                storefile,
                err
            );
        }
        futures::future::join_all(futs.into_iter()).map(|_| ())
    });
    core.run(fut).unwrap();
}

fn handle_message(
    mx: &mut MatrixClient,
    rid: &String,
    sender: String,
    body: String,
    store: &mut TabStore,
    futs: &mut Vec<MatrixFuture<()>>,
) {
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
            futs.push(Box::new(mx.send_simple(rid, msg).map(|_| ())) as Box<_>);
        } else {
            futs.push(Box::new(mx.send_simple(rid, PAID_USAGE).map(|_| ()))
                as Box<_>);
        },
        Some("!balance") => {
            // send the balance
            futs.push(Box::new(mx.send_simple(rid, store.balance(rid)).map(|_| ())) as Box<_>);
        }
        Some("!rebalance") => {
            // update the balance
            futs.push(Box::new(
                mx.send_simple(rid, "Rebalancing accounts to 0 mean.")
                    .map(|_| ()),
            ) as Box<_>);
            store.rebalance(rid);
            // send it
            futs.push(Box::new(mx.send_simple(rid, store.balance(rid)).map(|_| ())) as Box<_>);
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
            futs.push(Box::new(mx.send_simple(rid, msg).map(|_| ())) as Box<_>);
        } else {
            futs.push(Box::new(mx.send_simple(rid, PAIDTO_USAGE).map(|_| ()))
                as Box<_>);
        },
        _ => {}
    }
}
