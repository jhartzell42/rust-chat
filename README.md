# rust-chat
Exercise where I wrote a little chat server using Rust.

Connect to it using netcat:
```bash
$ nc localhost 5000
```

It'll prompt you for a username and send your messages to everyone who has a username that doesn't match yours.
