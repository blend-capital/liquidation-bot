use anyhow::Result;
use std::fs::OpenOptions;
use std::io::{Error, Write};
use std::path::Path;

pub fn log_error(msg: &str, dir: &str) -> Result<(), Error> {
    let file_path = Path::new(dir).join("error_logs.txt");

    let mut output = OpenOptions::new()
        .append(true)
        .create(true)
        .open(file_path)?;
    writeln!(output, "{}", msg)?;
    output.flush()?;
    Ok(())
}

pub fn heartbeat(block: &u32, dir: &str) -> Result<(), Error> {
    let file_path = Path::new(dir).join("heartbeat.txt");

    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(file_path)?;

    write!(output, "{}", block)?;
    output.flush()?;
    Ok(())
}
