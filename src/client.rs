//rust TLS for TLS on handshake and socket + HTTP/HTTP_types for connecting on

use crate::frame::Frame;
use async_std::io::BufReader;
use async_std::prelude::*;
// use async_std::stream;
// use async_std::stream::StreamExt;
use async_tls::TlsConnector;
use http::{uri::Builder, Uri};

pub struct Client {
    pub stream: async_tls::client::TlsStream<async_std::net::TcpStream>,
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
        })
    }

    pub async fn close(&mut self, code: Option<u16>) -> Result<(), std::io::Error> {
        let frame = Frame::new_close(code, Some(0));
        self.stream.write_all(frame.as_bytes()).await
    }
    pub async fn ping(&mut self, data: Option<&[u8]>) -> Result<(), std::io::Error> {
        let frame = Frame::new_ping(data, Some(0));
        self.stream.write_all(frame.as_bytes()).await
    }
}
