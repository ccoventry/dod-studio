use super::*;
use crate::test_support::Scratch;
use std::io::{BufWriter, Read, Write};

#[test]
fn test_write_director_event_payload() {
    let scratch = Scratch::new("director_event_payload");
    let temp_path = scratch.join("payload.dem");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path)
        .unwrap();

    let mut writer = BufWriter::new(file);

    let time = 10.5_f32;
    let tick = 1050_i32;
    let command = "echo test";
    let info_block = vec![0u8; 8];

    write_director_event_payload(&mut writer, time, tick, &info_block, command).unwrap();
    writer.flush().unwrap();

    let mut file = std::fs::File::open(&temp_path).unwrap();
    let mut result = Vec::new();
    file.read_to_end(&mut result).unwrap();
    let _ = std::fs::remove_file(&temp_path);

    let cmd_bytes = command.as_bytes();
    let payload_len = (1 + cmd_bytes.len() + 1) as u8;

    let mut expected = Vec::new();
    expected.push(1_u8); // NetworkMessage type
    expected.extend_from_slice(&time.to_le_bytes());
    expected.extend_from_slice(&tick.to_le_bytes());
    expected.extend_from_slice(&info_block);

    let mut payload = Vec::new();
    payload.push(0x33); // svc_director
    payload.push(payload_len);
    payload.push(0x0A); // DRC_CMD_STUFFTEXT
    payload.extend_from_slice(cmd_bytes);
    payload.push(0x00); // null terminator

    let msg_len = (payload.len() + 1) as u32;
    expected.extend_from_slice(&msg_len.to_le_bytes());
    expected.extend_from_slice(&payload);
    expected.push(0x01); // svc_nop

    assert_eq!(result, expected);
}

#[test]
fn a_console_command_too_long_for_its_frame_is_an_error_not_a_panic() {
    assert!(check_console_cmd_len(&"a".repeat(63)).is_ok());
    let err = check_console_cmd_len(&"a".repeat(64)).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn too_long_commands_names_only_the_ones_that_do_not_fit() {
    let fits = "a".repeat(63);
    let too_long = "b".repeat(64);
    assert_eq!(
        crate::game_config::too_long_commands(&[fits, too_long.clone()]),
        vec![too_long]
    );
}
