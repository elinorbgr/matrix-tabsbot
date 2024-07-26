use std::sync::Arc;

use matrix_sdk::{event_handler::Ctx, ruma::events::room::{member::StrippedRoomMemberEvent, message::{MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent}}, Client, Room, RoomState};

use tokio::{sync::Mutex, time::{sleep, Duration}};

use crate::{tabs::{SearchError, TabStore}, utils::{format_amount, parse_amount}};

static PAID_USAGE: &str = r#"Usage: !paid <amount> <Any description you like>
<amount> must be positive, without units, using `.` as cent separator"#;

static PAIDTO_USAGE: &str = r#"Usage: !paidto <username> <amount> <Any description you like>
<amount> must be positive, without units, using `.` as cent separator"#;

pub async fn send_or_log_error(room: &Room, txt: impl Into<String>) {
    if let Err(e) = room.send(RoomMessageEventContent::text_plain(txt)).await {
        println!("Failed to send message to room {}: {}", room.room_id(), e);
    }
}

pub async fn on_stripped_state_member(
    room_member: StrippedRoomMemberEvent,
    client: Client,
    room: Room,
) {
    if room_member.state_key != client.user_id().unwrap() {
        return;
    }

    tokio::spawn(async move {
        println!("Autojoining room {}", room.room_id());
        let mut delay = 2;

        while let Err(err) = room.join().await {
            // retry autojoin due to synapse sending invites, before the
            // invited user can join for more information see
            // https://github.com/matrix-org/synapse/issues/4345
            println!("Failed to join room {} ({err:?}), retrying in {delay}s", room.room_id());

            sleep(Duration::from_secs(delay)).await;
            delay *= 2;

            if delay > 3600 {
                println!("Can't join room {} ({err:?})", room.room_id());
                break;
            }
        }
        println!("Successfully joined room {}", room.room_id());
    });
}

pub async fn on_room_message(event: OriginalSyncRoomMessageEvent, room: Room, context: Ctx<Arc<Mutex<TabStore>>>) {
    if room.state() != RoomState::Joined {
        return;
    }
    let MessageType::Text(text_content) = event.content.msgtype else {
        return;
    };

    let mut store = context.0.lock().await;

    let mut splits = text_content.body.split_whitespace();
    match splits.next() {
        Some("!paid") => if let Some(amount) = splits.next().and_then(parse_amount) {
            store.pay(amount, room.room_id(), &event.sender);
            let rest = splits.collect::<Vec<_>>();
            if let Err(e) = room.send_state_event(store.get(room.room_id()).unwrap().clone()).await {
                println!("Failed to to update tab state on room {}: {}", room.room_id(), e);
            }
            let msg = format!(
                "{} paid {} for \"{}\"",
                event.sender,
                format_amount(amount),
                rest.join(" ")
            );
            send_or_log_error(&room, msg).await;
        } else {
            send_or_log_error(&room, PAID_USAGE).await;
        },
        Some("!balance") => {
            send_or_log_error(&room, store.balance(room.room_id())).await;
        },
        Some("!rebalance") => {
            send_or_log_error(&room, "Rebalancing accounts to 0 mean.").await;
            store.rebalance(room.room_id());
            send_or_log_error(&room, store.balance(room.room_id())).await;
        },
        Some("!paidto") => if let (Some(txt), Some(amount)) = (splits.next(), splits.next().and_then(parse_amount)) {
            let msg = match store.payto(amount, room.room_id(), &event.sender, txt) {
                Ok(other) => {
                    let rest = splits.collect::<Vec<_>>();
                    if let Err(e) = room.send_state_event(store.get(room.room_id()).unwrap().clone()).await {
                        println!("Failed to to update tab state on room {}: {}", room.room_id(), e);
                    }
                    format!(
                        "{} paid {} to {} for \"{}\"",
                        event.sender,
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
            send_or_log_error(&room, msg).await;
        } else {
            send_or_log_error(&room, PAIDTO_USAGE).await;
        },
        _ => {}
    }

}