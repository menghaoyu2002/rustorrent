use std::fmt::Display;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

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
}

pub struct InvalidMessageIdError {
    id: u8,
}

impl Display for InvalidMessageIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid message id: {}", self.id)
    }
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
        }
    }

    pub fn from_value(id: u8) -> Result<MessageId, InvalidMessageIdError> {
        match id {
            0 => Ok(MessageId::Choke),
            1 => Ok(MessageId::Unchoke),
            2 => Ok(MessageId::Interested),
            3 => Ok(MessageId::NotInterested),
            4 => Ok(MessageId::Have),
            5 => Ok(MessageId::Bitfield),
            6 => Ok(MessageId::Request),
            7 => Ok(MessageId::Piece),
            8 => Ok(MessageId::Cancel),
            9 => Ok(MessageId::Port),
            _ => Err(InvalidMessageIdError { id }),
        }
    }
}

impl Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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

pub struct SendMessageError {
    message: Message,
    error: String,
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

    pub fn get_id(&self) -> Result<MessageId, InvalidMessageIdError> {
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

pub async fn send_message(
    stream: &mut TcpStream,
    message: Message,
) -> Result<(), SendMessageError> {
    stream
        .write_all(&message.serialize())
        .await
        .map_err(|e| SendMessageError {
            message: message.clone(),
            error: format!("Failed to send message: {}", e),
        })?;

    Ok(())
}

pub async fn receive_message(stream: &mut TcpStream) -> Result<Message, SendMessageError> {
    let mut len = [0u8; 4];
    stream
        .read_exact(&mut len)
        .await
        .map_err(|e| SendMessageError {
            message: Message::new(MessageId::Choke, &Vec::new()),
            error: format!("Failed to read message length: {}", e),
        })?;

    let len = u32::from_be_bytes(len);
    let mut message = vec![0u8; len as usize];
    stream
        .read_exact(&mut message)
        .await
        .map_err(|e| SendMessageError {
            message: Message::new(MessageId::Choke, &Vec::new()),
            error: format!("Failed to read message: {}", e),
        })?;

    let id = message[0];
    let payload = message[1..].to_vec();

    Ok(Message { len, id, payload })
}
