use yaws::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();
    // client connect, websocket.org uses weak ciphers and rustls will refuse to connect securely
    let client = Client::connect_insecure(format!("ws://echo.websocket.org"))
        .await
        .unwrap();
    let (mut reader, mut writer) = client.split();
    let read_task = tokio::spawn(async move {
        loop {
            match reader.read_message().await {
                Ok(msg) => {
                    println!("Received message: {:?}", msg);
                    if msg.is_close() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    });
    //can't spawn 2 write task, so use a stream combiner.where we can copy
    let write_task = tokio::spawn(async move {
        for counter in 0..25u8 {
            match writer.send_str(format!("hello {}", counter)).await {
                Ok(_) => {}
                e => {
                    return e;
                }
            }
            if counter % 5 == 0 {
                std::thread::sleep_ms(250);
            };
            // tokio::task::yield_now().aw+ait;
        }
        writer.send_close(Some(1000)).await
    });
    read_task.await??;
    write_task.await??;
    //does not neatly handly server-initiated close.. should think about that
    Ok(())
}
