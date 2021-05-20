
use bytes::{BytesMut};
use std::error::Error;
use std::io::{ErrorKind};
use std::str::from_utf8;
use std::sync::{Arc, Mutex};

use tokio::net::{TcpListener, TcpStream};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast;

#[derive (Clone, Debug)]
struct Message {
    username: String,
    message: String,
}

pub struct BufferedReader<T> {
    stream: T,
    buffer: BytesMut,
    eof: bool,
}

impl<T: AsyncReadExt + Unpin> BufferedReader<T> {
    pub fn new(stream: T) -> Self {
        BufferedReader {
            stream: stream,
            buffer: BytesMut::new(),
            eof: false,
        }
    }

    pub async fn read_line(&mut self) -> Result<Option<String>, Box<dyn Error>> {
        loop {
            for (i, ch) in self.buffer.iter().enumerate() {
                if *ch == b'\n' {
                    return Ok(Some(String::from(from_utf8(&*self.buffer.split_to(i + 1))?)))
                }
            }

            if self.eof {
                return if self.buffer.len() == 0 {
                    Ok(None)
                } else {
                    return Ok(Some(String::from(from_utf8(&*self.buffer.split())?)))
                }
            }

            if let 0 = self.stream.read_buf(&mut self.buffer).await? {
                self.eof = true;
            }
            println!("Completed read. Buffer contents: {:?}", self.buffer);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Bind the listener to the address
    let listener = TcpListener::bind("127.0.0.1:5000").await?;
    let (tx, _) = broadcast::channel(16);
    let tx = Arc::new(Mutex::new(tx));
    loop {
        // The second item contains the IP and port of the new connection.
        let (socket, _) = listener.accept().await?;

        let tx = tx.clone();
        let rx = {
            let tx = tx.lock().unwrap();
            tx.subscribe()
        };

        tokio::spawn(async move {
            handle_connection(tx, rx, socket).await.unwrap_or_else(|err| {
                eprintln!("Error: {}", err);
                ()
            });
        });

    }
}

async fn handle_connection(tx: Arc<Mutex<broadcast::Sender<Message>>>,
                           rx: broadcast::Receiver<Message>,
                           tcp_stream: TcpStream) -> Result<(), Box<dyn Error>> {
    let (read_socket, mut write_socket) = io::split(tcp_stream);
    write_socket.write_all(b"username: ").await?;

    let mut buff_reader = BufferedReader::new(read_socket);

    let username = match buff_reader.read_line().await? {
        None => { return Ok(()); }
        Some(username) => String::from(username.trim())
    };
    let username_clone = username.clone();

    tokio::spawn(async move {
        writer_loop(write_socket, String::from(username), rx).await.unwrap_or_else(|err| {
            eprintln!("Error: {}", err);
            ()
        });
    });

    reader_loop(buff_reader, String::from(username_clone), tx).await
}

async fn writer_loop<T: AsyncWriteExt + Unpin>(mut write_socket: T,
                                               username: String,
                                               mut rx: broadcast::Receiver<Message>) -> Result<(), Box<dyn Error>> {
    loop {
        let msg : Message = rx.recv().await?;

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

async fn reader_loop<T: AsyncReadExt + Unpin>(mut conn: BufferedReader<T>,
                                              username: String,
                                              tx: Arc<Mutex<broadcast::Sender<Message>>>) -> Result<(), Box<dyn Error>> {
    loop {
        match conn.read_line().await? {
            None => {
                println!("Done with this socket");
                return Ok(());
            },
            Some(line) => {
                println!("GOT: {:?}", line);
                let tx = tx.lock().unwrap();
                tx.send(Message { username: username.clone(), message: line })?;
            },
        }
    }
}
