use std::fmt::Debug;

use bytes::Bytes;

pub type DataCursor = u64;

#[derive(Debug, PartialEq, Clone)]
pub struct TurboMessage {
    /// Similar to TCP sequence number.
    pub write: DataCursor,
    /// Similar to TCP acknowledgment number.
    pub read: DataCursor,
    pub command: Command,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Command {
    Request(Request),
    Response(Response),
    Reset,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Request {
    Forward(Payload),
    Rewind,
    Shut,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Response {
    Forward(Result),
    Rewind(Result),
    Shut(Result),
}

impl Response {
    pub fn is_error(&self) -> bool {
        match self {
            Response::Forward(result) => result.is_error(),
            Response::Rewind(result) => result.is_error(),
            Response::Shut(result) => result.is_error(),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Result {
    Ok,
    Error,
}

impl Result {
    pub fn is_error(&self) -> bool {
        match self {
            Result::Ok => false,
            Result::Error => true,
        }
    }
}

#[derive(PartialEq, Clone)]
pub struct Payload {
    /// The application data payload bytes.
    pub data: Bytes,
}

impl From<Bytes> for Payload {
    fn from(value: Bytes) -> Self {
        Payload { data: value }
    }
}

impl Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Payload(len: {})", self.data.len())
    }
}

impl TurboMessage {
    fn request(write: DataCursor, read: DataCursor, request: Request) -> Self {
        Self {
            write,
            read,
            command: Command::Request(request),
        }
    }

    fn response(write: DataCursor, read: DataCursor, response: Response) -> Self {
        Self {
            write,
            read,
            command: Command::Response(response),
        }
    }

    pub fn reset(write: DataCursor, read: DataCursor) -> Self {
        Self {
            write,
            read,
            command: Command::Reset,
        }
    }

    pub fn forward(write: DataCursor, read: DataCursor, payload: Bytes) -> Self {
        Self::request(write, read, Request::Forward(payload.into()))
    }

    pub fn rewind(write: DataCursor, read: DataCursor) -> Self {
        Self::request(write, read, Request::Rewind)
    }

    pub fn shut(write: DataCursor, read: DataCursor) -> Self {
        Self::request(write, read, Request::Shut)
    }

    pub fn forward_ok(write: DataCursor, read: DataCursor) -> Self {
        Self::response(write, read, Response::Forward(Result::Ok))
    }

    pub fn rewind_ok(write: DataCursor, read: DataCursor) -> Self {
        Self::response(write, read, Response::Rewind(Result::Ok))
    }

    pub fn shut_ok(write: DataCursor, read: DataCursor) -> Self {
        Self::response(write, read, Response::Shut(Result::Ok))
    }

    pub fn forward_err(write: DataCursor, read: DataCursor) -> Self {
        Self::response(write, read, Response::Forward(Result::Error))
    }

    pub fn rewind_err(write: DataCursor, read: DataCursor) -> Self {
        Self::response(write, read, Response::Rewind(Result::Error))
    }

    pub fn shut_err(write: DataCursor, read: DataCursor) -> Self {
        Self::response(write, read, Response::Shut(Result::Error))
    }

    pub fn is_error(&self) -> bool {
        match &self.command {
            Command::Response(response) => response.is_error(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {}
