//rust TLS for TLS on handshake and socket + HTTP/HTTP_types for connecting on

use crate::frame::Frame;
use http::{uri::Builder, Uri};
use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, rustls::ClientConfig, TlsConnector};

use tokio::io::{
    split, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf,
};

#[derive(Clone, Debug)]
pub struct Client<S> {
    stream: S,
    read_buffer: Vec<u8>,
    read_buffer_head: usize,
    parse_buffer_head: usize,
}

impl Client<TcpStream> {
    pub async fn connect_insecure<U: Into<String>>(uri: U) -> Result<Self, std::io::Error> {
        //TODO infer port from scheme if missing. if scheme is missing default to wss://?
        let uri: Uri = uri.into().parse().unwrap();
        let host = uri.host().unwrap();
        let scheme = match uri.scheme_str() {
            Some("ws") | None => "ws",
            Some(scheme) => panic!("Invalid scheme \"{}\" in uri! ", scheme),
        };
        let authority = if uri.port_u16().is_some() {
            uri.authority().unwrap().as_str().to_owned()
        } else {
            let mut authority = host.to_owned();
            authority.push(':');
            authority.push_str(match scheme {
                "wss" => "443",
                "ws" => "80",
                _ => unreachable!(),
            });
            authority
        };
        let path_and_query = uri.path_and_query().unwrap();
        let uri = Builder::new()
            .authority(authority.as_str())
            .path_and_query(path_and_query.as_str())
            .scheme(scheme)
            .build()
            .unwrap();
        //https://docs.rs/async-tls/0.7.0/async_tls/struct.TlsConnector.html
        // println!("Connecting to {}", uri);
        let mut stream = TcpStream::connect(uri.authority().unwrap().as_str()).await?;
        // println!("Connected to authority");
        //let (sink, source) = encrypted_stream.split();
        let upgrade_request = format!(
            "GET {} HTTP/1.1\r
Host: {}\r
Connection: Upgrade\r
Upgrade: websocket\r
Sec-Websocket-Version: 13\r
Sec-Websocket-Key: {}\r\n\r\n",
            uri.path_and_query().unwrap(),
            authority,
            "dGhlIHNhbXBsZSBub25jZQ=="
        );
        // println!("request\n=======\n{}", upgrade_request);
        stream.write_all(upgrade_request.as_bytes()).await?;
        let mut header_pairs = std::collections::HashMap::new();
        let mut buffered = BufReader::new(stream);
        {
            let mut line = String::new();
            buffered.read_line(&mut line).await?;
            // println!("received header line: {}", line);
            let split = line.split(" ").collect::<Vec<_>>();
            assert_eq!("HTTP/1.1", split[0]);
            assert_eq!("101", split[1]);
            line.clear();
            while buffered.read_line(&mut line).await? > 2 {
                let split = line.splitn(2, ":").collect::<Vec<_>>();
                // println!("Key: \"{}\"", split[0].to_lowercase());
                // println!("Value: \"{}\"", split[1].trim());
                //todo: header can have duplicate keys.
                assert_eq!(
                    None,
                    header_pairs.insert(
                        split[0].to_lowercase().to_owned(),
                        split[1].trim().to_owned(),
                    )
                );
                line.clear();
            }
        }
        let content_length = header_pairs
            .get("content_length")
            .map(|s| s.parse::<usize>().unwrap())
            .unwrap_or(0);
        //todo skip body
        assert_eq!(content_length, 0);
        let vec = buffered.buffer().to_owned();
        Ok(Client {
            stream: buffered.into_inner(),
            read_buffer_head: vec.len(),
            read_buffer: vec,
            parse_buffer_head: 0,
        })
    }
}

pub type SecureClient = Client<TlsStream<TcpStream>>;

impl Client<TlsStream<TcpStream>> {
    pub async fn connect_secure<U: Into<String>>(uri: U) -> Result<Self, std::io::Error> {
        //TODO infer port from scheme if missing. if scheme is missing default to wss://?
        let uri: Uri = uri.into().parse().unwrap();
        let host = uri.host().unwrap();
        let scheme = match uri.scheme_str() {
            Some("wss") | None => "wss",
            Some(scheme) => panic!("Invalid scheme \"{}\" in uri! ", scheme),
        };
        let authority = if uri.port_u16().is_some() {
            uri.authority().unwrap().as_str().to_owned()
        } else {
            let mut authority = host.to_owned();
            authority.push(':');
            authority.push_str(match scheme {
                "wss" => "443",
                "ws" => "80",
                _ => unreachable!(),
            });
            authority
        };
        let path_and_query = uri.path_and_query().unwrap();
        let uri = Builder::new()
            .authority(authority.as_str())
            .path_and_query(path_and_query.as_str())
            .scheme(scheme)
            .build()
            .unwrap();
        //https://docs.rs/async-tls/0.7.0/async_tls/struct.TlsConnector.html
        //println!("Connecting to {}", uri);
        let tcp_stream = TcpStream::connect(uri.authority().unwrap().as_str()).await?;
        //println!("Connected to authority");
        let mut config = ClientConfig::new();
        // println!(
        //     "dns {:?} from {}",
        //     webpki::DNSNameRef::try_from_ascii_str(host),
        //     host
        // );
        config
            .root_store
            .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
        //println!("{:?}", config.ciphersuites);
        let connector = TlsConnector::from(std::sync::Arc::new(config));
        let mut encrypted_stream = connector
            .connect(
                webpki::DNSNameRef::try_from_ascii_str(host).unwrap(),
                tcp_stream,
            )
            .await?;
        //println!("encryption added");
        //let (sink, source) = encrypted_stream.split();
        let upgrade_request = format!(
            "GET {} HTTP/1.1\r
Host: {}\r
Connection: Upgrade\r
Upgrade: websocket\r
Sec-Websocket-Version: 13\r
Sec-Websocket-Key: {}\r\n\r\n",
            uri.path_and_query().unwrap(),
            authority,
            "dGhlIHNhbXBsZSBub25jZQ=="
        );
        //println!("request\n=======\n{}", upgrade_request);
        encrypted_stream
            .write_all(upgrade_request.as_bytes())
            .await?;
        let mut header_pairs = std::collections::HashMap::new();
        let mut buffered = BufReader::new(encrypted_stream);
        {
            let mut line = String::new();
            buffered.read_line(&mut line).await?;
            //println!("received header line: {}", line);
            let split = line.split(" ").collect::<Vec<_>>();
            assert_eq!("HTTP/1.1", split[0]);
            assert_eq!("101", split[1]);
            line.clear();
            while buffered.read_line(&mut line).await? > 2 {
                let split = line.splitn(2, ":").collect::<Vec<_>>();
                //println!("Key: \"{}\"", split[0].to_lowercase());
                //println!("Value: \"{}\"", split[1].trim());
                //todo: header can have duplicate keys.
                assert_eq!(
                    None,
                    header_pairs.insert(
                        split[0].to_lowercase().to_owned(),
                        split[1].trim().to_owned(),
                    )
                );
                line.clear();
            }
        }
        let content_length = header_pairs
            .get("content_length")
            .map(|s| s.parse::<usize>().unwrap())
            .unwrap_or(0);
        //todo skip body
        assert_eq!(content_length, 0);
        Ok(Client {
            stream: buffered.into_inner(),
            read_buffer: vec![0u8; 4096], //needs to be atleast 2+8(+4)
            read_buffer_head: 0,
            parse_buffer_head: 0,
        })
    }
}
impl<Stream: std::marker::Unpin + AsyncWriteExt> Client<Stream> {
    pub async fn send_close(&mut self, code: Option<u16>) -> Result<(), std::io::Error> {
        let frame = Frame::new_close(code, Some(0));
        self.stream.write_all(frame.as_bytes()).await
    }
    //todo: when writting the server, we don't need to set the mask so use vectored write op to save on copying
    //maybe give the option to use mask=0 and use that here too. That would save us a copy.
    pub async fn ping(&mut self, data: Option<&[u8]>) -> Result<(), std::io::Error> {
        let frame = Frame::new_ping(data, Some(0));
        self.stream.write_all(frame.as_bytes()).await
    }

    pub async fn pong(&mut self, data: Option<&[u8]>) -> Result<(), std::io::Error> {
        let frame = Frame::new_pong(data, Some(0));
        self.stream.write_all(frame.as_bytes()).await
    }

    pub async fn send_str<S: AsRef<str>>(&mut self, msg: S) -> Result<(), std::io::Error> {
        let frame = Frame::new_text(msg, Some(0));
        //println!("Sending frame: {:?}", frame);
        self.stream.write_all(frame.as_bytes()).await
    }
    pub async fn send_binary(&mut self, msg: &[u8]) -> Result<(), std::io::Error> {
        let max_frame_size: usize = 1 << 16;
        if msg.len() < max_frame_size {
            let frame = Frame::new_binary(&msg, Some(0), true);
            self.stream.write_all(frame.as_bytes()).await
        } else {
            let frame = Frame::new_binary(&msg[0..max_frame_size], Some(0), false);
            self.stream.write_all(frame.as_bytes()).await?;
            for start in (max_frame_size..msg.len()).step_by(max_frame_size) {
                let frame =
                    Frame::new_continuation(&msg[start..start + max_frame_size], Some(0), false);
                self.stream.write_all(frame.as_bytes()).await?;
            }
            let frame = Frame::new_continuation(&[], Some(0), true);
            self.stream.write_all(frame.as_bytes()).await?;
            Ok(())
        }
    }
}

impl<Stream: std::marker::Unpin + AsyncReadExt> Client<Stream> {
    pub async fn wait_on_close(&mut self) -> Result<(), std::io::Error> {
        while match self.read_message().await?.opcode() {
            crate::frame::Opcode::Close => false,
            _ => true,
        } { /*spin here*/ }
        Ok(())
    }
    //rename to handle error or something, do resize when a read fills up the buffer.
    // means buffer isn't big enough to cache all incoming messages in single read call.
    fn resize_buffer(&mut self, e: crate::frame::WsParsingError) {
        match e {
            crate::frame::WsParsingError::IncompleteHeader => {
                //header is always small, preemptively move it back to the front of buffer
                self.read_buffer
                    .copy_within(self.parse_buffer_head..self.read_buffer_head, 0);
                self.read_buffer_head = self.read_buffer_head - self.parse_buffer_head;
                self.parse_buffer_head = 0;
            }
            crate::frame::WsParsingError::IncompleteMessage(missing_bytes) => {
                // println!(
                //     "missing {} bytes and {} free",
                //     missing_bytes,
                //     self.read_buffer.len() - self.read_buffer_head
                // );
                if missing_bytes > self.read_buffer.len() - self.read_buffer_head {
                    //message is not going to fit here, move it back to the start
                    self.read_buffer
                        .copy_within(self.parse_buffer_head..self.read_buffer_head, 0);
                    self.read_buffer_head = self.read_buffer_head - self.parse_buffer_head;
                    self.parse_buffer_head = 0;
                }
                //if it still doesn't fit, in the buffer...
                if missing_bytes > self.read_buffer.len() - self.read_buffer_head {
                    // println!("resizing to {}", self.read_buffer_head + missing_bytes);
                    self.read_buffer
                        .resize_with(self.read_buffer_head + missing_bytes, u8::default);
                }
            }
        }
    }
    // check if there's a valid message between parse and read head.
    // if there is,
    fn peek_frame_from_buffer(&mut self) -> Option<&Frame> {
        if let Err(e) =
            { Frame::parse_slice(&self.read_buffer[self.parse_buffer_head..self.read_buffer_head]) }
        {
            self.resize_buffer(e);
            None
        } else {
            let frame = unsafe {
                Frame::from_slice_unchecked(
                    &self.read_buffer[self.parse_buffer_head..self.read_buffer_head],
                )
            };
            Some(frame)
        }
    }
    pub async fn read_message(&mut self) -> Result<&Frame, std::io::Error> {
        //maybe return usize of frame size instead on succes? easier to do borrows cause drops dependence.
        while let None = self.peek_frame_from_buffer() {
            // println!("starting read from {}", self.read_buffer_head);
            let bytes_read = self
                .stream
                .read(&mut self.read_buffer[self.read_buffer_head..])
                .await?;
            // println!("\t Read {} bytes", bytes_read);
            //shortcut to zero if we've reached eof.
            if bytes_read == 0 {
                // println!("Connection got closed!");
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
            }
            self.read_buffer_head = self.read_buffer_head + bytes_read;
        }
        let frame = unsafe {
            // println!(
            //     "Making a frame from {} to {}",
            //     self.parse_buffer_head, self.read_buffer_head
            // );
            Frame::from_slice_unchecked(
                &self.read_buffer[self.parse_buffer_head..self.read_buffer_head],
            )
        };
        if self.read_buffer_head == self.parse_buffer_head + frame.len() {
            self.read_buffer_head = 0;
            self.parse_buffer_head = 0;
        } else {
            self.parse_buffer_head += frame.len();
        }
        Ok(frame)
    }
}

pub type SecureReader = Client<ReadHalf<TlsStream<TcpStream>>>;
pub type SecureWriter = Client<WriteHalf<TlsStream<TcpStream>>>;

impl<Stream: std::marker::Unpin + AsyncReadExt + AsyncWriteExt> Client<Stream> {
    pub fn split(self) -> (Client<ReadHalf<Stream>>, Client<WriteHalf<Stream>>) {
        let (read, write) = split(self.stream);
        (
            Client {
                stream: read,
                read_buffer: self.read_buffer,
                read_buffer_head: self.read_buffer_head,
                parse_buffer_head: self.parse_buffer_head,
            },
            Client {
                stream: write,
                read_buffer: vec![],
                read_buffer_head: 0,
                parse_buffer_head: 0,
            },
        )
    }

    pub async fn close(&mut self, code: Option<u16>) -> Result<(), std::io::Error> {
        self.send_close(code).await?;
        self.wait_on_close().await
    }
}
