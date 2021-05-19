
use bytes::{Bytes, BytesMut};
use std::error::Error;
use std::io::{ErrorKind};
use std::str::from_utf8;
use std::sync::{Arc, Mutex};

use tokio::net::{TcpListener};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::sync::broadcast;

/*
struct Message {
    username: String,
    message: Bytes,
}
*/

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
    let listener = TcpListener::bind("127.0.0.1:5000").await.unwrap();
    let (tx, _) = broadcast::channel::<Bytes>(16);
    let tx = Arc::new(Mutex::new(tx));
    loop {
        // The second item contains the IP and port of the new connection.
        let (socket, _) = listener.accept().await.unwrap();
        let (read_socket, write_socket) = io::split(socket);

        let tx = tx.clone();
        let rx = {
            let tx = tx.lock().unwrap();
            tx.subscribe()
        };

        tokio::spawn(async move {
            reader(read_socket, tx).await.unwrap();
        });

        tokio::spawn(async move {
            writer(write_socket, rx).await.unwrap();
        });
    }
}

async fn writer<T: AsyncWriteExt + Unpin>(mut write_socket: T, mut rx: broadcast::Receiver<Bytes>) -> Result<(), Box<dyn Error>> {
    loop {
        let buf : Bytes = rx.recv().await?;
        let mut mut_buf : BytesMut = BytesMut::new();
        mut_buf.extend_from_slice(&*buf);
        match write_socket.write_all_buf(&mut mut_buf).await {
            Ok(()) => (),
            Err(err) => {
                match err.kind() {
                    ErrorKind::BrokenPipe => (),
                    ErrorKind::ConnectionReset => (),
                    ErrorKind::NotConnected => (),
                    _ => { return Err(err.into()) }
                }
            }
        }
    }
}

async fn reader<T: AsyncReadExt + Unpin>(read_socket: T, tx: Arc<Mutex<broadcast::Sender<Bytes>>>) -> Result<(), Box<dyn Error>> {
    let mut conn = BufferedReader::new(read_socket);
    loop {
        match  conn.read_line().await? {
            None => {
                println!("Done with this socket");
                return Ok(());
            },
            Some(line) => {
                println!("GOT: {:?}", line);
                let tx = tx.lock().unwrap();
                tx.send(line.into())?;
            },
        }
    }
}
