
mod frame_reader;
use crate::frame_reader::{Delimitation, FrameReader, ReadFrameError, Frameable};

use bytes::{Bytes, BytesMut};
use color_eyre::eyre::{Report, WrapErr, eyre};

use std::io::{ErrorKind};
use std::str::{Utf8Error, from_utf8};
use std::sync::{Arc, Mutex};

use tokio::net::{TcpListener, TcpStream};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast;

#[derive (Clone, Debug)]
struct Message {
    username: String,
    message: String,
}

#[derive (Clone, Debug)]
pub struct Line(String);

impl Frameable for Line {
    type ParseError = Utf8Error;
    fn delimit(buf: &BytesMut) -> Delimitation {
        for (i, ch) in buf.iter().enumerate() {
            if *ch == b'\n' {
                return Delimitation::Index(i + 1);
            }
        }
        Delimitation::NotPresent
    }

    fn parse(buf: Bytes) -> Result<Self, Self::ParseError> {
        Ok(Line(String::from(from_utf8(&*buf)?)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Report> {
    color_eyre::install()?;

    let listener = TcpListener::bind("127.0.0.1:5000").await.wrap_err("bind")?;
    let (tx, _) = broadcast::channel(16);
    let tx = Arc::new(Mutex::new(tx));
    loop {
        // The second item contains the IP and port of the new connection.
        let (socket, _) = listener.accept().await.wrap_err("accept")?;

        let tx = tx.clone();
        let rx = {
            let tx = tx.lock().map_err(|_| eyre!("Poison lock"))?;
            tx.subscribe()
        };

        tokio::spawn(async move {
            // TODO: These next two lines are duplicated
            handle_connection(tx, rx, socket).await.unwrap_or_else(|err| {
                eprintln!("Error:\n{:?}", err);
                ()
            });
        });

    }
}

async fn handle_connection(tx: Arc<Mutex<broadcast::Sender<Message>>>,
                           rx: broadcast::Receiver<Message>,
                           tcp_stream: TcpStream) -> Result<(), Report> {
    let (read_socket, mut write_socket) = io::split(tcp_stream);
    write_socket.write_all(b"username: ").await?;

    let mut buff_reader = FrameReader::new(read_socket);

    let Line(username) = buff_reader.read_frame().await.wrap_err("Retrieving username")?;
    let username = String::from(username.trim());
    if username.len() == 0 {
        return Err(eyre!("Need to specify a username"));
    }

    let username_clone = username.clone();

    tokio::spawn(async move {
        writer_loop(write_socket, String::from(username), rx).await.unwrap_or_else(|err| {
            eprintln!("Error:\n{:?}", err);
            ()
        });
    });

    reader_loop(buff_reader, String::from(username_clone), tx).await
}

async fn writer_loop<T: AsyncWriteExt + Unpin>(mut write_socket: T,
                                               username: String,
                                               mut rx: broadcast::Receiver<Message>) -> Result<(), Report> {
    loop {
        let msg : Message = rx.recv().await.wrap_err("Receiving message for broadcast")?;

        if msg.username == username {
            continue;
        }

        let to_send = format!("[{}] {}", msg.username, msg.message);

        let mut mut_buf : BytesMut = BytesMut::new();
        mut_buf.extend_from_slice(to_send.as_bytes());
        match write_socket.write_all_buf(&mut mut_buf).await {
            Ok(()) => (),
            Err(err) => {
                match err.kind() {
                    ErrorKind::BrokenPipe => return Ok(()),
                    ErrorKind::ConnectionReset => return Ok(()),
                    ErrorKind::NotConnected => return Ok(()),
                    _ => { return Err(err.into()) }
                }
            }
        }
    }
}

async fn reader_loop<T: AsyncReadExt + Unpin>(mut conn: FrameReader<Line, T>,
                                              username: String,
                                              tx: Arc<Mutex<broadcast::Sender<Message>>>) -> Result<(), Report> {
    loop {
        match conn.read_frame().await {
            Err(ReadFrameError::EOF) => {
                println!("Done with this socket");
                return Ok(());
            },
            Err(other) => {
                Err(other).wrap_err("Reading message")?
            }
            Ok(Line(line)) => {
                println!("GOT: {:?}", line);
                let tx = tx.lock().map_err(|_| eyre!("Poison lock"))?;
                tx.send(Message { username: username.clone(), message: line })
                    .wrap_err("Sending message through channel")?;
            },
        }
    }
}
