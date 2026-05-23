use thiserror::Error;

use crate::domain::{
    cache::{Cache, CacheError},
    command::Command,
};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpStream,
};

#[derive(Debug, Error)]
pub enum ReplError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Cache(#[from] CacheError),
}

#[derive(Debug)]
pub struct Session {
    id: u32,
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    cache: Cache,
}

impl Session {
    pub fn new(id: u32, stream: TcpStream, cache: Cache) -> Result<Self, std::io::Error> {
        let writer = stream.try_clone()?;
        let reader = BufReader::new(stream);
        Ok(Self {
            id,
            reader,
            writer,
            cache,
        })
    }

    pub fn get_message(&mut self) -> Result<Option<String>, std::io::Error> {
        let mut input = String::new();
        match self.reader.read_line(&mut input) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(input)),
            Err(e) => Err(e),
        }
    }

    pub fn get_command(&mut self) -> Result<Option<Command>, std::io::Error> {
        loop {
            match self.get_message() {
                Ok(Some(message)) => match Command::try_from(message.as_str()) {
                    Ok(command) => return Ok(Some(command)),
                    Err(e) => {
                        self.writer
                            .write_all(format!("Failed to parse command: {}\n", e).as_bytes())?;
                        self.writer.flush()?;
                        continue;
                    }
                },
                Err(e) => {
                    self.writer
                        .write_all(format!("Failed to get message: {}\n", e).as_bytes())?;
                    self.writer.flush()?;
                    continue;
                }
                Ok(None) => return Ok(None),
            };
        }
    }

    pub fn execute(&mut self, command: Command) -> Result<(), CacheError> {
        let mut response = match command {
            Command::Get { key } => match self.cache.get(&key) {
                Ok(Some(value)) => format!("{:?} => {:?}", key, value),
                Ok(None) => format!("{:?} not found", key),
                Err(e) => format!("Failed to get key: {}", e),
            },
            Command::Set { key, value } => match self.cache.set(key.as_slice(), value.as_slice()) {
                Ok(_) => format!("{:?} => {:?}", key, value),
                Err(e) => format!("Failed to set key to value: {}", e),
            },
            Command::Delete { key } => match self.cache.delete(&key) {
                Ok(Some(_)) => format!("{:?} deleted", key),
                Ok(None) => format!("{:?} not found", key),
                Err(e) => format!("Failed to delete key: {}", e),
            },
        };
        response.push('\n');
        self.writer.write_all(response.as_bytes())?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn repl(&mut self) -> Result<(), ReplError> {
        loop {
            match self.get_command() {
                Ok(Some(command)) => match self.execute(command) {
                    Ok(_) => (),
                    Err(e) => {
                        self.writer
                            .write_all(format!("Failed execute command: {}", e).as_bytes())?;
                        continue;
                    }
                },
                Ok(None) => {
                    println!("Client {} disconnected", self.id);
                    break;
                }
                Err(e) => {
                    self.writer
                        .write_all(format!("Failed get command: {}", e).as_bytes())?;
                    continue;
                }
            }
        }
        Ok(())
    }
}
