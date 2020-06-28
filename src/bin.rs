//rust TLS for TLS on handshake and socket + HTTP/HTTP_types for connecting on
//use std::io::{Read, Write};
use async_std::prelude::*;
use yaws::Client;
pub fn main() -> Result<(), std::io::Error> {
    async_std::task::block_on(async {
        let mut client = Client::connect_on("wss://gateway.discord.gg/?v=6&encoding=json")
            .await
            .unwrap();
        println!("connected!");
        println!("awaiting intial message..");
        println!("{:?}", client.read_message().await.unwrap());
        println!("Sendding a ping");
        client.ping(Some(&[1, 2, 3, 4])).await.unwrap();
        println!("{:?}", client.read_message().await.unwrap());
        println!("Sending a close!");
        client.close(Some(1000)).await.unwrap();
        println!("Close send!");
        println!("{:?}", client.read_message().await.unwrap());
        let mut buffer = [0u8; 1];
        let bytes_read = client.stream.read(&mut buffer).await.unwrap();
        if bytes_read == 0 {
            println!("connection closed by server");
        } else {
            println!("connection STILL OPEN");
            println!("{:?}", client.read_message().await.unwrap());
        }
        // {
        //     println!("Sending messaeg  {:?}", message_frame);
        //     stream.write(message_frame.as_bytes());
        //     let bytes_read = stream.read(&mut buffer).unwrap();
        //     // let response = std::str::from_utf8(&buffer[0..bytes_read]).unwrap();
        //     println!(
        //         "got response: {:?} => {:?}",
        //         response_frame,
        //         std::str::from_utf8(response_frame.masked_data())
        //     );
        // }
        // {
        //     let bytes = close_frame.as_bytes().len();
        //     println!("Sending message of length {:?}", close_frame);
        //     let msg = close_frame.as_bytes();
        //     println!("{:?}", msg);
        //     stream.write(msg);
        //     let bytes_read = stream.read(&mut buffer).unwrap();
        //     let response_frame = unsafe { yaws::Frame::from_slice_unchecked(&buffer) };
        //     println!("{:?}", response_frame);
        //     // let response = std::str::from_utf8(&buffer[0..bytes_read]).unwrap();
        //     let data = response_frame.masked_data();
        //     let close_code = data[1] as u16 | ((data[0] as u16) << 8);
        //     println!("got response: {:?}", close_code);
        // }
        //}
    });
    Ok(())
}
