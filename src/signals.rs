use std::{thread, process::exit};
use signal_hook::{ consts::SIGINT, iterator::Signals};

pub fn start() {
    let mut signals = Signals::new(&[SIGINT]).expect("error creating signal handler");

    thread::spawn(move || {
        for sig in signals.forever() {
            eprintln!("Received signal {:?}", sig);
            match sig {
                SIGINT => exit(0),
                _ => {},
            }
        }
    });
}