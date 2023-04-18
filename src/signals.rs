use std::{thread, process::exit};
use signal_hook::{ consts::{SIGINT,SIGALRM}, iterator::Signals};
use std::sync::mpsc::SyncSender;

pub enum SignalMessage {
    SessionExpired,
}

pub fn start(signal_tx: SyncSender<SignalMessage>) {
    let mut signals = Signals::new(&[SIGINT,SIGALRM]).expect("error creating signal handler");

    thread::spawn(move || {
        for sig in signals.forever() {
            eprintln!("Received signal {:?}", sig);
            match sig {
                SIGINT => exit(0),
                SIGALRM => signal_tx.send(SignalMessage::SessionExpired).unwrap(),
                _ => {},
            }
        }
    });
}