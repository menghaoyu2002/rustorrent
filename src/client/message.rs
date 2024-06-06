use std::fmt::Display;

use tokio::{io::AsyncReadExt, net::TcpStream, task::yield_now};

pub enum MessageId {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
    Port = 9,
    KeepAlive = 10,
}

impl MessageId {
    pub fn value(&self) -> u8 {
        match self {
            MessageId::Choke => 0,
            MessageId::Unchoke => 1,
            MessageId::Interested => 2,
            MessageId::NotInterested => 3,
            MessageId::Have => 4,
            MessageId::Bitfield => 5,
            MessageId::Request => 6,
            MessageId::Piece => 7,
            MessageId::Cancel => 8,
            MessageId::Port => 9,
            MessageId::KeepAlive => 10,
        }
    }

    pub fn from_value(id: u8) -> MessageId {
        match id {
            0 => MessageId::Choke,
            1 => MessageId::Unchoke,
            2 => MessageId::Interested,
            3 => MessageId::NotInterested,
            4 => MessageId::Have,
            5 => MessageId::Bitfield,
            6 => MessageId::Request,
            7 => MessageId::Piece,
            8 => MessageId::Cancel,
            9 => MessageId::Port,
            10 => MessageId::KeepAlive,
            _ => unreachable!("unhandled message id value: {}", id),
        }
    }
}

impl Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageId::KeepAlive => write!(f, "KeepAlive"),
            MessageId::Choke => write!(f, "Choke"),
            MessageId::Unchoke => write!(f, "Unchoke"),
            MessageId::Interested => write!(f, "Interested"),
            MessageId::NotInterested => write!(f, "NotInterested"),
            MessageId::Have => write!(f, "Have"),
            MessageId::Bitfield => write!(f, "Bitfield"),
            MessageId::Request => write!(f, "Request"),
            MessageId::Piece => write!(f, "Piece"),
            MessageId::Cancel => write!(f, "Cancel"),
            MessageId::Port => write!(f, "Port"),
        }
    }
}

#[derive(Debug)]
pub struct SendMessageError {
    message: Message,
    error: String,
}

#[derive(Debug)]
pub struct ReceiveMessageError {
    error: String,
}

#[derive(Debug)]
pub enum ReceiveError {
    ReceiveError(ReceiveMessageError),
    WouldBlock,
}

pub enum SendError {
    SendError(SendMessageError),
    WouldBlock,
}

impl Display for ReceiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReceiveError::ReceiveError(e) => write!(f, "Failed to receive message: {}", e.error),
            ReceiveError::WouldBlock => write!(f, "Would block"),
        }
    }
}

impl Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::SendError(e) => write!(f, "Failed to send message: {}", e.error),
            SendError::WouldBlock => write!(f, "Would block"),
        }
    }
}

impl Display for SendMessageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Failed to send message: message = {}, error = {}",
            self.message, self.error
        )
    }
}

#[derive(Debug)]
pub struct Message {
    len: u32,
    id: u8,
    payload: Vec<u8>,
}

impl Message {
    pub fn new(id: MessageId, payload: &Vec<u8>) -> Self {
        Self {
            len: payload.len() as u32 + 1, // +1 for the id
            id: id.value(),
            payload: payload.clone(),
        }
    }

    pub fn get_id(&self) -> MessageId {
        MessageId::from_value(self.id)
    }

    pub fn get_payload(&self) -> &Vec<u8> {
        &self.payload
    }

    fn serialize(&self) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(&self.len.to_be_bytes());
        message.push(self.id);
        message.extend_from_slice(&self.payload);
        message
    }
}

impl Clone for Message {
    fn clone(&self) -> Self {
        Self {
            len: self.len,
            id: self.id,
            payload: self.payload.clone(),
        }
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Message {{ len: {}, id: {}, payload: {:?} }}",
            self.len, self.id, self.payload
        )
    }
}

pub async fn send_message(stream: &TcpStream, message: &Message) -> Result<(), SendError> {
    let mut bytes_written = 0;
    while bytes_written < message.serialize().len() {
        stream.writable().await.unwrap();
        match stream.try_write(&message.serialize()) {
            Ok(0) => {
                return Err(SendError::SendError(SendMessageError {
                    message: message.clone(),
                    error: "EOF".to_string(),
                }))
            }
            Ok(n) => {
                bytes_written += n;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(SendError::WouldBlock);
            }
            Err(e) => {
                return Err(SendError::SendError(SendMessageError {
                    message: message.clone(),
                    error: format!("Failed to send message: {}", e),
                }));
            }
        };
    }
    Ok(())
}

pub async fn receive_message(stream: &TcpStream) -> Result<Message, ReceiveError> {
    let mut len = [0u8; 4];
    let mut bytes_read = 0;
    while bytes_read < 4 {
        stream.readable().await.unwrap();
        match stream.try_read(&mut len) {
            Ok(0) => {
                return Err(ReceiveError::ReceiveError(ReceiveMessageError {
                    error: "stream was closed".to_string(),
                }))
            }
            Ok(n) => {
                bytes_read += n;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(ReceiveError::WouldBlock);
            }
            Err(e) => {
                return Err(ReceiveError::ReceiveError(ReceiveMessageError {
                    error: format!("Failed to read message length: {}", e),
                }));
            }
        }
    }
    let len = u32::from_be_bytes(len);
    if len == 0 {
        return Ok(Message {
            len,
            id: MessageId::KeepAlive.value(),
            payload: Vec::new(),
        });
    }

    let mut message = Vec::new();
    let mut bytes_read = 0;
    while bytes_read < len as usize {
        let mut buffer = vec![0u8; len as usize];
        stream.readable().await.unwrap();
        match stream.try_read(&mut buffer) {
            Ok(0) => {
                return Err(ReceiveError::ReceiveError(ReceiveMessageError {
                    error: "stream was closed".to_string(),
                }))
            }
            Ok(n) => {
                bytes_read += n;
                message.extend_from_slice(&buffer[..n]);
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                yield_now().await;
            }
            Err(e) => {
                return Err(ReceiveError::ReceiveError(ReceiveMessageError {
                    error: format!("Failed to read message: {}", e),
                }));
            }
        }
    }
    let id = message[0];
    let payload = message[1..].to_vec();

    Ok(Message { len, id, payload })
}
