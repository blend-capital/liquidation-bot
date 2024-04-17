use anyhow::Result;
use std::env;
use std::fs::OpenOptions;
use std::io::{Error, Write};

pub fn log_error(msg: &str) -> Result<(), Error> {
    let file_path = env::current_dir().unwrap().join("error_logs.txt");

    let mut output = OpenOptions::new()
        .append(true)
        .create(true)
        .open(file_path)?;
    writeln!(output, "{}", msg)?;
    output.flush().unwrap();
    Ok(())
}
