use anyhow::Result;
use std::env;
use std::fs::OpenOptions;
use std::io::{Error, Write};

pub fn log_error(msg: &str) -> Result<(), Error> {
    let file_path = env::current_dir()?.join("error_logs.txt");

    let mut output = OpenOptions::new()
        .append(true)
        .create(true)
        .open(file_path)?;
    writeln!(output, "{}", msg)?;
    output.flush()?;
    Ok(())
}

pub fn heartbeat(block: &u32) -> Result<(), Error> {
    let file_path = env::current_dir()?.join("heartbeat.txt");

    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;
    writeln!(output, "{}", block)?;
    output.flush()?;
    Ok(())
}
