//rust TLS for TLS on handshake and socket + HTTP/HTTP_types for connecting on
//use std::io::{Read, Write};
use async_std::prelude::*;
use yaws::Client;
pub fn main() -> Result<(), std::io::Error> {
    let use_json = true;
    async_std::task::block_on(async {
        let mut client = Client::connect_on(format!(
            "wss://gateway.discord.gg/?v=6&encoding={}f&compress=zlib-stream",
            if use_json { "json" } else { "etf" }
        ))
        .await
        .unwrap();
        println!("connected!");
        println!("awaiting intial message..");
        let message = client.read_message().await.unwrap();
        println!("{:?}", message);
        match message {
            yaws::client::WsResponse::Binary(input) => {
                let cmf = input[0];
                let flg = input[1];
                println!(
                    "compression method: {}\ncompression info: {}",
                    cmf & 0x0F,
                    (cmf & 0xF0) >> 4
                );
                println!(
                    "FDICT: {}\nlevel: {}",
                    (flg & (1 << 5)) != 0,
                    (flg >> 6) & 3
                );
                let mut decomp = flate2::Decompress::new(true);
                let mut out = [0u8; 1 << 14];
                // decomp.set_dictionary(&[0u8]);
                println!(
                    "{:?}",
                    decomp
                        .decompress(&input, &mut out, flate2::FlushDecompress::None)
                        .unwrap(),
                );
                let out_size = decomp.total_out() as usize;
                println!("Decompressed {} to {} bytes", decomp.total_in(), out_size);
                println!("decompressed to: {:?}", &out[0..out_size]);
                if use_json {
                    let text = std::str::from_utf8(&out[0..out_size]).unwrap();
                    println!("{:?}", text);
                } else {
                    let etf = erlang_term::parse::from_bytes(&out[0..out_size]);
                    println!("{:?}", etf);
                }
            }
            _ => {
                println!("not binary!");
            }
        }
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
