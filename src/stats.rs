use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread;

use super::CargoRequest;

pub fn stat_collector() -> SyncSender<CargoRequest> {
    let (sender, receiver) = sync_channel::<CargoRequest>(10);
    thread::spawn(move || loop {
        if let Ok(req) = receiver.recv() {
            info!("Logging a crate request to sqlite: {:?}", req);
        } else {
            break;
        }
    });
    sender
}
