//rust TLS for TLS on handshake and socket + HTTP/HTTP_types for connecting on

use crate::frame::Frame;
use async_std::io::BufReader;
use async_std::net::TcpStream;
use async_std::prelude::*;
use async_tls::client::TlsStream;
use async_tls::TlsConnector;
use http::{uri::Builder, Uri};
pub struct Client {
    pub stream: TlsStream<TcpStream>,
    read_buffer: Vec<u8>,
    read_buffer_head: usize,
    parse_buffer_head: usize,
}
//todo: handle paartials, esp. wrt text
#[derive(Debug)]
pub enum WsResponse<'a> {
    Binary(&'a [u8]),
    Text(&'a str),
    Close(&'a [u8]), //TODO: close can be zero and if not must have a code.
    Ping(&'a [u8]),
    Pong(&'a [u8]),
    InvalidOpcode(&'a [u8]),
}

impl Client {
    pub async fn connect_on<U: Into<String>>(uri: U) -> Result<Self, std::io::Error> {
        //TODO infer port from scheme if missing. if scheme is missing default to wss://?
        let uri: Uri = uri.into().parse().unwrap();
        let host = uri.host().unwrap();
        let scheme = match uri.scheme_str() {
            Some("wss") => "wss",
            Some("ws") => "ws",
            Some(scheme) => panic!(
                "Invalid scheme \"{}\" in uri! should be one of \"ws\", \"wss\"",
                scheme
            ),
            None => unreachable!(),
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
        println!("Connecting to {}", uri);
        let tcp_stream =
            async_std::net::TcpStream::connect(uri.authority().unwrap().as_str()).await?;
        println!("Connected to authority");
        let connector = TlsConnector::default();
        let mut encrypted_stream = connector.connect(host, tcp_stream).await?;
        println!("encryption added");
        let upgrade_request = format!(
            "GET {} HTTP/1.1\r
Host: {}\r
Connection: Upgrade\r
Upgrade: websocket\r
Sec-Websocket-Version: 13\r
Sec-Websocket-Key: {}\r\n\r\n",
            uri.path_and_query().unwrap(),
            host,
            "dGhlIHNhbXBsZSBub25jZQ=="
        );
        println!("request\n=======\n{}", upgrade_request);
        encrypted_stream
            .write_all(upgrade_request.as_bytes())
            .await?;
        let mut header_pairs = std::collections::HashMap::new();
        let mut buffered = BufReader::new(encrypted_stream);
        {
            let mut line = String::new();
            buffered.read_line(&mut line).await?;
            println!("received header line: {}", line);
            let split = line.split(" ").collect::<Vec<_>>();
            assert_eq!("HTTP/1.1", split[0]);
            assert_eq!("101", split[1]);
            line.clear();
            while buffered.read_line(&mut line).await? > 2 {
                let split = line.splitn(2, ":").collect::<Vec<_>>();
                println!("Key: \"{}\"", split[0].to_lowercase());
                println!("Value: \"{}\"", split[1].trim());
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
            read_buffer: vec![0u8; 512], //needs to be atleast 2+8(+4)
            read_buffer_head: 0,
            parse_buffer_head: 0,
        })
    }

    pub async fn close(&mut self, code: Option<u16>) -> Result<(), std::io::Error> {
        let frame = Frame::new_close(code, Some(0));
        self.stream.write_all(frame.as_bytes()).await
    }
    //todo: when writting the server, we don't need to set the mask so use vectored write op to save on copying
    //maybe give the option to use mask=0 and use that here too. That would save us a copy.
    pub async fn ping(&mut self, data: Option<&[u8]>) -> Result<(), std::io::Error> {
        let frame = Frame::new_ping(data, Some(0));
        self.stream.write_all(frame.as_bytes()).await
    }

    //first read 2 bytes into a fixed buffer.
    //then read up to 2 more bytes to determine real size
    pub async fn read_message(&mut self) -> Result<WsResponse<'_>, std::io::Error> {
        while let Err(e) = {
            //bug: this can fail if read_buffer_head is at the end of stream.
            let bytes_read = self
                .stream
                .read(&mut self.read_buffer[self.read_buffer_head..])
                .await?;
            //shortcut to zero if we've reached eof.
            if bytes_read == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
            }
            self.read_buffer_head = self.read_buffer_head + bytes_read;
            Frame::parse_slice(&self.read_buffer[self.parse_buffer_head..self.read_buffer_head])
        } {
            match e {
                crate::frame::WsParsingError::IncompleteHeader => {
                    //header is always small, preemptively move it back to the front of buffer
                    self.read_buffer
                        .copy_within(self.parse_buffer_head..self.read_buffer_head, 0);
                    self.read_buffer_head = self.read_buffer_head - self.parse_buffer_head;
                    self.parse_buffer_head = 0;
                }
                crate::frame::WsParsingError::IncompleteMessage(missing_bytes) => {
                    if missing_bytes > self.read_buffer.len() - self.read_buffer_head {
                        //message is not going to fit here, move it back to the start
                        self.read_buffer
                            .copy_within(self.parse_buffer_head..self.read_buffer_head, 0);
                        self.read_buffer_head = self.read_buffer_head - self.parse_buffer_head;
                        self.parse_buffer_head = 0;
                    }
                    //if it still doesn't fit, in the buffer...
                    if missing_bytes > self.read_buffer.len() - self.read_buffer_head {
                        self.read_buffer
                            .resize_with(self.read_buffer_head + missing_bytes, u8::default)
                    }
                }
            }
        }
        //if we get here, then we know for certain we can cast the buffer to a frame
        // so this is safe.
        let frame = unsafe {
            Frame::from_slice_unchecked(
                &self.read_buffer[self.parse_buffer_head..self.read_buffer_head],
            )
        };
        //if there's no data in our buffer, after this frame, we reset the read and parse head to zero.
        // otherwise we move the parse head up.
        if self.read_buffer_head == self.parse_buffer_head + frame.len() {
            self.read_buffer_head = 0;
        }
        self.parse_buffer_head = self.read_buffer_head;
        Ok(match frame.opcode() {
            crate::frame::Opcode::Binary => WsResponse::Binary(frame.masked_data()),
            crate::frame::Opcode::Text => {
                WsResponse::Text(std::str::from_utf8(frame.masked_data()).unwrap())
            }
            crate::frame::Opcode::Ping => WsResponse::Ping(frame.masked_data()),
            crate::frame::Opcode::Pong => WsResponse::Pong(frame.masked_data()),
            crate::frame::Opcode::Close => WsResponse::Close(frame.masked_data()),
            crate::frame::Opcode::Invalid(_) => WsResponse::InvalidOpcode(frame.masked_data()),
        })
    }
}
