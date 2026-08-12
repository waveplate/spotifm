use chrono::{Local, SecondsFormat};
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::{Mutex, OnceLock},
};

static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();

pub fn init(path: &Path) -> io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    LOG_FILE.set(Mutex::new(file)).map_err(|_| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Spotifm file logging was already initialized",
        )
    })?;

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        write_line("PANIC", &panic_info.to_string());
        previous_hook(panic_info);
    }));
    Ok(())
}

pub fn write_line(level: &str, message: &str) {
    let Some(file) = LOG_FILE.get() else {
        return;
    };
    let Ok(mut file) = file.lock() else {
        return;
    };
    let timestamp = Local::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let _ = writeln!(file, "{timestamp} [{level}] {message}");
}
