
use bytes::{Bytes, BytesMut};

use tokio::io::AsyncReadExt;

use std::marker::PhantomData;
use std::fmt::Debug;
use std::io::Error;

pub enum Delimitation {
    Index(usize),
    NotPresent,
}

pub trait Frameable {
    type ParseError;
    fn delimit(buf: &BytesMut) -> Delimitation;
    fn parse(buf: Bytes) -> Result<Self, Self::ParseError> where Self: Sized;
}

#[derive (thiserror::Error, Debug)]
pub enum ReadFrameError<T: Frameable + Debug> where <T as Frameable>::ParseError: Debug {
    #[error("Parsing")]
    ParseError(T::ParseError),
    #[error("IO Error")]
    IOError(#[from] Error),
    #[error("EOF")]
    EOF,
    #[error("EOF with leftover data")]
    EOFWithData(Bytes),
}

pub struct FrameReader<T, Stream> {
    stream: Stream,
    buffer: BytesMut,
    eof: bool,
    _marker: PhantomData<T>,
}

impl<T: Frameable + Debug, Stream: AsyncReadExt + Unpin> FrameReader<T, Stream> {
    pub fn new(stream: Stream) -> Self {
        FrameReader {
            stream: stream,
            buffer: BytesMut::new(),
            eof: false,
            _marker: PhantomData,
        }
    }

    pub async fn read_frame(&mut self) -> Result<T, ReadFrameError<T>> where <T as Frameable>::ParseError: Debug {
        loop {
            match T::delimit(&self.buffer) {
                Delimitation::Index(size) => {
                    return Ok(T::parse(self.buffer
                                       .split_to(size)
                                       .freeze()).map_err(|err| ReadFrameError::ParseError(err))?);
                },
                Delimitation::NotPresent => (),
            }

            if self.eof {
                return if self.buffer.len() == 0 {
                    Err(ReadFrameError::EOF)
                } else {
                    Err(ReadFrameError::EOFWithData(self.buffer.split().freeze()))
                }
            }

            if let 0 = self.stream.read_buf(&mut self.buffer).await? {
                self.eof = true;
            }
        }
    }
}
