use yaws::Client;

async fn do_case(case: usize) -> Result<Option<u16>, std::io::Error> {
    let mut close_code = None;
    let client = Client::connect_insecure(format!(
        "ws://localhost:9001/runCase?case={}&agent=yaws",
        case
    ))
    .await
    .unwrap();
    let (mut reader, mut writer) = client.split();
    let mut response_buffer = vec![];
    let mut current_continue = None;
    while let Ok(frame) = reader.read_message().await {
        //check if message is valid
        // then check if is control, handle immidiatly
        // if not control check if in continue stream rn, assert that same type
        // if not respond and/or set continue stream properly
        if !frame.is_valid() {
            writer.send_close(Some(1002)).await?;
            reader.wait_on_close().await?;
            return Ok(Some(1002));
        } else {
            // println!("{:?}", frame);
            match frame.opcode() {
                yaws::Opcode::Binary => {
                    if current_continue.is_some() {
                        writer.send_close(Some(1002)).await?;
                        reader.wait_on_close().await?;
                        return Ok(Some(1002));
                    } else if !frame.is_final() {
                        current_continue = Some(frame.opcode());
                        response_buffer = frame.masked_data().to_owned();
                    } else {
                        writer.send_binary(frame.masked_data()).await?
                    }
                }
                yaws::Opcode::Text => {
                    if current_continue.is_some() {
                        writer.send_close(Some(1002)).await?;
                        reader.wait_on_close().await?;
                        return Ok(Some(1002));
                    } else if !frame.is_final() {
                        current_continue = Some(frame.opcode());
                        response_buffer = frame.masked_data().to_owned();
                    } else {
                        let message_str = std::str::from_utf8(frame.masked_data());
                        if message_str.is_err() {
                            writer.send_close(Some(1007)).await?;
                            reader.wait_on_close().await?;
                            return Ok(Some(1007));
                        } else {
                            writer.send_str(message_str.unwrap()).await?
                        }
                    }
                }
                yaws::Opcode::Continue => {
                    if current_continue.is_none() {
                        writer.send_close(Some(1002)).await?;
                        reader.wait_on_close().await?;
                        return Ok(Some(1002));
                    }
                    response_buffer.extend_from_slice(frame.masked_data());
                    if frame.is_final() {
                        match current_continue {
                            Some(yaws::Opcode::Text) => {
                                current_continue = None;
                                let message_str = std::str::from_utf8(&response_buffer);
                                if message_str.is_err() {
                                    writer.send_close(Some(1007)).await?;
                                    reader.wait_on_close().await?;
                                    return Ok(Some(1007));
                                } else {
                                    writer.send_str(message_str.unwrap()).await?
                                }
                            }
                            Some(yaws::Opcode::Binary) => {
                                current_continue = None;
                                writer.send_binary(&response_buffer).await?
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                yaws::Opcode::Ping => {
                    writer.pong(Some(&frame.masked_data())).await?;
                }
                yaws::Opcode::Pong => {}
                yaws::Opcode::Close => {
                    let data = frame.masked_data();
                    if data.len() >= 2 {
                        close_code = Some(u16::from_be_bytes([data[0], data[1]]));
                        writer.send_close(close_code).await?;
                        return Ok(close_code);
                    } else {
                        // println!("closing with none");
                        writer.send_close(None).await?;
                        return Ok(close_code);
                    }
                }
                yaws::Opcode::Invalid(_) => {
                    writer.send_close(Some(1000)).await?;
                }
            }
        }
    }
    Ok(close_code)
}

async fn do_cases(id: usize, cases: Vec<usize>) -> Vec<Result<Option<u16>, std::io::Error>> {
    let mut ret = vec![];
    for case in cases {
        tokio::task::yield_now().await;
        let result = do_case(case).await;
        println!("({:2}) {} => {:?}", id, case, result);
        ret.push(result);
    }
    ret
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    pretty_env_logger::init();
    //don't set this to high, or tests will fail as they wait for other tasks to finish
    let n_tasks = 4;
    let handles = (0..n_tasks)
        .map(|task_id| {
            let my_cases = (1 + task_id..=517).step_by(n_tasks).collect::<Vec<_>>();
            tokio::spawn(do_cases(task_id, my_cases))
        })
        .collect::<Vec<_>>();
    for (idx, task) in handles.into_iter().enumerate() {
        let result = task.await;
        println!("handle completed with {} => {:?}", idx, result);
    }
    Ok(())
}
