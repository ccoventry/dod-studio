use std::{ffi::OsStr, fs::OpenOptions, io::Read, path::Path};

use nom::{
    Parser,
    bytes::complete::take,
    combinator::{map, verify},
    multi::{count, many_till, many0},
    number::complete::{le_f32, le_i8, le_i16, le_i32, le_u8, le_u16, le_u32},
};

use crate::{
    nom_helper::{Result, nom_fail, take_point_float},
    parse_netmsg,
    types::{
        Aux, AuxRefCell, ClientData, ConsoleCommand, Demo, DemoBuffer, DemoInfo, Directory,
        DirectoryEntry, Event, EventArgs, Frame, FrameData, Header, MessageData,
        MessageDataParseMode, MoveVars, NetworkMessage, NetworkMessageType, RefParams,
        SequenceInfo, Sound, UserCmd, WeaponAnimation,
    },
};

impl Demo {
    /// It is faster to parse netmessage because it just is for some reasons
    pub fn parse_from_file(
        path: impl AsRef<OsStr> + AsRef<Path>,
        netmsg_parse_mode: MessageDataParseMode,
    ) -> eyre::Result<Self> {
        let mut file = OpenOptions::new().read(true).open(path)?;
        let mut bytes: Vec<u8> = vec![];

        file.read_to_end(&mut bytes)?;

        Self::parse_from_bytes(bytes.as_slice(), netmsg_parse_mode)
    }

    pub fn parse_from_bytes(
        demo_bytes: &[u8],
        netmsg_parse_mode: MessageDataParseMode,
    ) -> eyre::Result<Self> {
        match parse_demo(demo_bytes, netmsg_parse_mode) {
            Ok((_, demo)) => Ok(demo),
            Err(err) => Err(eyre::eyre!("cannot parse demo: {}", err)),
        }
    }

    /// [`Demo::parse_from_bytes`] for a demo that may be damaged (#811): the
    /// frames up to the first one that does not parse, and where and why it
    /// stopped ([`parse_demo_tolerant`]). Only for reading what a demo holds:
    /// the patcher and the writer keep to the strict parse, which refuses a
    /// damaged file.
    pub fn parse_from_bytes_tolerant(
        demo_bytes: &[u8],
        netmsg_parse_mode: MessageDataParseMode,
    ) -> eyre::Result<(Self, Option<ParseDamage>)> {
        parse_demo_tolerant(demo_bytes, netmsg_parse_mode)
            .map_err(|reason| eyre::eyre!("cannot parse demo: {reason}"))
    }
}

/// Where a tolerant parse ([`parse_demo_tolerant`]) stopped reading a damaged
/// demo, and why (#811). Everything before it was read as the strict parse
/// reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseDamage {
    /// The directory entry the break is in, counting from 0, as the demo is
    /// returned: its last entry.
    pub entry_index: usize,
    /// That entry's description ("LOADING", "Playback").
    pub entry_description: String,
    /// Index of the frame that does not parse, counting every frame of every
    /// entry from 0: the number of frames read before it.
    pub frame_index: usize,
    /// The last frame read's own time (`Frame::time`), in seconds; `None`
    /// when the break is the entry's first frame.
    pub last_frame_time: Option<f32>,
    /// Where in the file the frame that does not parse starts.
    pub byte_offset: usize,
    /// From there to the end of the frames (the directory, or the end of the
    /// file): what was not read.
    pub bytes_unread: usize,
    /// Why it does not parse, in words.
    pub reason: String,
}

type NomErr<'a> = nom::Err<nom::error::Error<&'a [u8]>>;

/// The words for a parse error, never its byte dump. `nom_fail` keeps its
/// message in the error's `input`; any other error's `input` is whatever was
/// left to read, possibly megabytes, so only its kind is named.
fn plain_reason(err: &NomErr, file: &[u8]) -> String {
    use nom::error::ErrorKind;
    let e = match err {
        nom::Err::Incomplete(_) => return "the file ends partway through a frame".to_string(),
        nom::Err::Error(e) | nom::Err::Failure(e) => e,
    };
    // Edition 2021: no `let` chains in this crate.
    if e.code == ErrorKind::Fail && !e.input.is_empty() && e.input.len() <= 200 {
        if let Ok(text) = std::str::from_utf8(e.input) {
            if !text.chars().any(char::is_control) {
                return text.to_string();
            }
        }
    }
    let file_end = file.as_ptr() as usize + file.len();
    let at_file_end = e.input.as_ptr() as usize + e.input.len() == file_end;
    match e.code {
        ErrorKind::Eof if at_file_end => "the file ends partway through a frame".to_string(),
        ErrorKind::Eof => "a frame's data is cut short".to_string(),
        code => format!("a frame does not parse ({code:?})"),
    }
}

/// [`parse_demo`] that keeps what it read when a frame fails (#811). A frame
/// that does not parse ends its directory entry and the demo there: the
/// entries after it are dropped, and the returned [`ParseDamage`] says where
/// and why. Cutting a demo short is safe to read, since deltas only refer
/// back (`docs/goldsrc_demo_salvage.md`). A directory that is missing or does
/// not parse (a file cut short loses it, since it is written last) is read as
/// an unfinalised demo's: frame by frame from the header, up to where the
/// directory starts when that is inside the file.
///
/// `Err` (the reason, in words) when not even one frame parses, or the header
/// does not.
pub fn parse_demo_tolerant(
    i: &[u8],
    netmsg_parse_mode: MessageDataParseMode,
) -> std::result::Result<(Demo, Option<ParseDamage>), String> {
    let aux2 = Aux::new2();
    let file_start = i;
    aux2.borrow_mut().parsing_source_len = Some(file_start.len());

    let (frames_start, header) = parse_header(i).map_err(|e| plain_reason(&e, file_start))?;
    let walk = TolerantWalk {
        file_start,
        netmsg_parse_mode,
        aux: aux2.clone(),
    };

    // The frames end where the directory starts: at the end of the file with
    // no directory (an unfinalised demo writes offset 0) or one off the end
    // of a truncated file.
    let header_len = file_start.len() - frames_start.len();
    let offset = header.directory_offset;
    let bound =
        if offset > 0 && offset as usize >= header_len && offset as usize <= file_start.len() {
            offset as usize
        } else {
            file_start.len()
        };
    let (directory, damage) = match directory_headers(file_start, offset) {
        Some(entries) => walk.entries(entries, bound),
        None => walk.fallback(frames_start, bound),
    };

    let frames: usize = directory.entries.iter().map(|e| e.frames.len()).sum();
    if let (0, Some(damage)) = (frames, &damage) {
        return Err(damage.reason.clone());
    }

    {
        let mut aux = aux2.borrow_mut();
        aux.parsing_source_len = None;
        aux.parsed_from = Some((file_start.as_ptr() as usize, file_start.len()));
    }

    Ok((
        Demo {
            header,
            directory,
            _aux: Some(aux2),
        },
        damage,
    ))
}

/// A finalised demo's directory entries, frames not yet read, or `None` when
/// the directory is not in the file or does not parse. Each entry's frame
/// offset is checked against the file, as `parse_directory_entry` does.
fn directory_headers(file_start: &[u8], offset: i32) -> Option<Vec<DirectoryEntry>> {
    if offset <= 0 || offset as usize > file_start.len() {
        return None;
    }
    let (mut i, entry_count) =
        le_u32::<_, nom::error::Error<&[u8]>>(&file_start[offset as usize..]).ok()?;
    let mut entries = Vec::new();
    for _ in 0..entry_count {
        let (
            rest,
            (
                type_,
                description,
                flags,
                cd_track,
                track_time,
                frame_count,
                frame_offset,
                file_length,
            ),
        ) = (
            le_i32::<_, nom::error::Error<&[u8]>>,
            take(64usize),
            le_i32,
            le_i32,
            le_f32,
            le_i32,
            le_i32,
            le_i32,
        )
            .parse(i)
            .ok()?;
        if frame_offset < 0 || frame_offset as usize > file_start.len() {
            return None;
        }
        i = rest;
        entries.push(DirectoryEntry {
            type_,
            description: description.into(),
            flags,
            cd_track,
            track_time,
            frame_count,
            frame_offset,
            file_length,
            frames: vec![],
        });
    }
    Some(entries)
}

/// What [`parse_demo_tolerant`] reads frames with.
struct TolerantWalk<'a> {
    file_start: &'a [u8],
    netmsg_parse_mode: MessageDataParseMode,
    aux: AuxRefCell,
}

/// How one run of frames ended.
enum RunEnd {
    /// At a `NextSection` frame (kept), or at the bound.
    Clean,
    /// At a frame that does not parse, which starts this many bytes before
    /// the end of the file.
    Broken { left: usize, reason: String },
}

impl<'a> TolerantWalk<'a> {
    fn offset_of(&self, rest: &[u8]) -> usize {
        self.file_start.len() - rest.len()
    }

    /// Reads frames from `i` into `frames` until a `NextSection` frame (when
    /// `to_next_section`), the bound, or a frame that does not parse.
    fn run(
        &self,
        mut i: &'a [u8],
        bound: usize,
        to_next_section: bool,
        frames: &mut Vec<Frame>,
    ) -> (&'a [u8], RunEnd) {
        loop {
            if self.offset_of(i) >= bound && !to_next_section {
                return (i, RunEnd::Clean);
            }
            match parse_frame(i, self.netmsg_parse_mode, self.aux.clone()) {
                Ok((rest, frame)) => {
                    let is_next_section = matches!(frame.frame_data, FrameData::NextSection);
                    frames.push(frame);
                    i = rest;
                    if is_next_section && to_next_section {
                        return (i, RunEnd::Clean);
                    }
                }
                Err(e) => {
                    return (
                        i,
                        RunEnd::Broken {
                            left: i.len(),
                            reason: plain_reason(&e, self.file_start),
                        },
                    );
                }
            }
        }
    }

    fn damage(
        &self,
        entries: &[DirectoryEntry],
        left: usize,
        bound: usize,
        reason: String,
    ) -> ParseDamage {
        let byte_offset = self.file_start.len() - left;
        let last = entries.last();
        ParseDamage {
            entry_index: entries.len().saturating_sub(1),
            // Up to the first NUL: an entry built here has none.
            entry_description: last
                .map(|e| {
                    let bytes = &e.description.0;
                    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                    String::from_utf8_lossy(&bytes[..end]).into_owned()
                })
                .unwrap_or_default(),
            frame_index: entries.iter().map(|e| e.frames.len()).sum(),
            last_frame_time: last.and_then(|e| e.frames.last()).map(|f| f.time),
            byte_offset,
            bytes_unread: bound.saturating_sub(byte_offset),
            reason,
        }
    }

    /// A finalised demo's entries, each read up to its `NextSection`.
    /// What is left unread runs to `bound`, the directory.
    fn entries(
        &self,
        headers: Vec<DirectoryEntry>,
        bound: usize,
    ) -> (Directory, Option<ParseDamage>) {
        let mut entries: Vec<DirectoryEntry> = Vec::with_capacity(headers.len());
        for mut entry in headers {
            let start = &self.file_start[entry.frame_offset as usize..];
            let (_, end) = self.run(start, 0, true, &mut entry.frames);
            entries.push(entry);
            if let RunEnd::Broken { left, reason } = end {
                let damage = self.damage(&entries, left, bound, reason);
                return (Directory { entries }, Some(damage));
            }
        }
        (Directory { entries }, None)
    }

    /// An unfinalised demo's two entries, as [`parse_fallback_directory`]
    /// builds them, from `frames_start` up to byte `bound`.
    fn fallback(&self, frames_start: &'a [u8], bound: usize) -> (Directory, Option<ParseDamage>) {
        let entry = |type_, description: &str, start: &[u8], end: &[u8], frames: Vec<Frame>| {
            DirectoryEntry {
                type_,
                description: description.into(),
                flags: -1,
                cd_track: -1,
                track_time: 0.,
                frame_count: frames.len() as i32,
                frame_offset: self.offset_of(start) as i32,
                file_length: (start.len() - end.len()) as i32,
                frames,
            }
        };

        let mut loading = Vec::new();
        let (i, end) = self.run(frames_start, bound, true, &mut loading);
        let mut entries = vec![entry(0, "LOADING", frames_start, i, loading)];
        if let RunEnd::Broken { left, reason } = end {
            let damage = self.damage(&entries, left, bound, reason);
            return (Directory { entries }, Some(damage));
        }

        let mut playback = Vec::new();
        let (rest, end) = self.run(i, bound, false, &mut playback);
        entries.push(entry(1, "Playback", i, rest, playback));
        let damage = match end {
            RunEnd::Clean => None,
            RunEnd::Broken { left, reason } => Some(self.damage(&entries, left, bound, reason)),
        };
        (Directory { entries }, damage)
    }
}

pub fn parse_demo(i: &[u8], netmsg_parse_mode: MessageDataParseMode) -> Result<Demo> {
    let aux2 = Aux::new2();

    let file_start = i;

    // Every slice the frame parsers see from here on is a suffix of this
    // buffer, which is what lets a network message work out its payload's
    // offset from its own remaining length. See `parse_network_messages`.
    aux2.borrow_mut().parsing_source_len = Some(file_start.len());

    let (i, header) = parse_header(i)?;

    let (i, directory) = if header.directory_offset == 0 {
        let frames_start = i;

        parse_fallback_directory(frames_start, file_start, netmsg_parse_mode, aux2.clone())
    } else {
        // `directory_offset` comes straight off disk and is signed, so a
        // corrupted file can make this index negative (which wraps to an
        // enormous `usize`) or simply point past EOF. Slicing on it unchecked
        // panics, and with `panic = "abort"` in the release profile a panic
        // here takes the whole process down -- a folder scan cannot skip the
        // file and carry on, because there is nothing left to carry on with.
        // See #225.
        let offset = header.directory_offset;
        if offset < 0 || offset as usize > file_start.len() {
            return nom_fail(format!(
                "directory offset {} is outside the file ({} bytes)",
                offset,
                file_start.len()
            ));
        }
        let directory_start = &file_start[offset as usize..];

        parse_directory(directory_start, file_start, netmsg_parse_mode, aux2.clone())
    }?;

    {
        let mut aux = aux2.borrow_mut();
        aux.parsing_source_len = None;
        aux.parsed_from = Some((file_start.as_ptr() as usize, file_start.len()));
    }

    Ok((
        i,
        Demo {
            header,
            directory,
            _aux: Some(aux2),
        },
    ))
}

pub fn parse_header(i: &[u8]) -> Result<Header> {
    let (i, magic) = take(8usize)(i)?;

    if magic != "HLDEMO\x00\x00".as_bytes() {
        return nom_fail(format!("magic is not HLDEMO: `{:?}`", magic));
    }

    map(
        (
            le_i32,
            le_i32,
            take(260usize),
            take(260usize),
            le_u32,
            le_i32,
        ),
        |(
            demo_protocol,
            network_protocol,
            map_name,
            game_directory,
            map_checksum,
            directory_offset,
        ): (_, _, &[u8], &[u8], _, _)| Header {
            magic: magic.to_vec(),
            demo_protocol,
            network_protocol,
            map_name: map_name.into(),
            game_directory: game_directory.into(),
            map_checksum,
            directory_offset,
        },
    )
    .parse(i)
}

pub fn parse_directory<'a>(
    i: &'a [u8],
    file_start: &'a [u8],
    netmsg_parse_mode: MessageDataParseMode,
    aux: AuxRefCell,
) -> Result<'a, Directory> {
    let (i, entry_count) = le_u32(i)?;

    let local_parse_directory_entry =
        |i| parse_directory_entry(i, file_start, netmsg_parse_mode, aux.clone());

    map(
        count(local_parse_directory_entry, entry_count as usize),
        |entries| Directory { entries },
    )
    .parse(i)
}

/// Parse a fallback directory for demo files that were not finalized by a client.
///
/// Unfinalized demos have the following differences from finalized demos:
///
/// - Directory metadata is not yet written
///   - Directory offset is 0
///   - Number of entries is 0
///   - Entry metadata is missing
///  - A terminating NextSection frame for the current entry is not yet written
///
/// This parser reconstructs the missing details from frame data so they can be organized into
/// individual directory entries.
pub fn parse_fallback_directory<'a>(
    frames_start: &'a [u8],
    file_start: &'a [u8],
    netmsg_parse_mode: MessageDataParseMode,
    aux: AuxRefCell,
) -> Result<'a, Directory> {
    let parser = |i| parse_frame(i, netmsg_parse_mode, aux.clone());

    let loading_entry_start = frames_start;

    let (i, (mut loading_frames, next_section_frame)) = many_till(
        parser,
        verify(parser, |frame| {
            matches!(frame.frame_data, FrameData::NextSection)
        }),
    )
    .parse(frames_start)?;

    loading_frames.push(next_section_frame);

    let loading_entry_end = i;

    let loading_entry = DirectoryEntry {
        type_: 0,
        description: "LOADING".into(),
        flags: -1,
        cd_track: -1,
        track_time: 0.,

        frame_count: loading_frames.len() as i32,
        frame_offset: (file_start.len() - loading_entry_start.len()) as i32,
        file_length: (loading_entry_start.len() - loading_entry_end.len()) as i32,

        frames: loading_frames,
    };

    let playback_entry_start = i;
    let (i, playback_frames) = many0(parser).parse(i)?;
    let playback_entry_end = i;

    let playback_entry = DirectoryEntry {
        type_: 1,
        description: "Playback".into(),
        flags: -1,
        cd_track: -1,
        track_time: 0.,

        frame_count: playback_frames.len() as i32,
        frame_offset: (file_start.len() - playback_entry_start.len()) as i32,
        file_length: (playback_entry_start.len() - playback_entry_end.len()) as i32,

        frames: playback_frames,
    };

    Ok((
        i,
        Directory {
            entries: vec![loading_entry, playback_entry],
        },
    ))
}

pub fn parse_directory_entry<'a>(
    i: &'a [u8],
    file_start: &'a [u8],
    netmsg_parse_mode: MessageDataParseMode,
    aux: AuxRefCell,
) -> Result<'a, DirectoryEntry> {
    let (
        end_of_current_directory_entry,
        (type_, description, flags, cd_track, track_time, frame_count, frame_offset, file_length),
    ) = (
        le_i32,
        take(64usize),
        le_i32,
        le_i32,
        le_f32,
        le_i32,
        le_i32,
        le_i32,
    )
        .parse(i)?;

    // frame_count is unreliable
    // parse until NextSection and stop for current entry
    let mut frames: Vec<Frame> = vec![];
    // Same as `directory_offset` above: signed, read from the file, and used
    // as an index into it. #225.
    if frame_offset < 0 || frame_offset as usize > file_start.len() {
        return nom_fail(format!(
            "directory entry frame offset {} is outside the file ({} bytes)",
            frame_offset,
            file_start.len()
        ));
    }
    let mut frames_start = &file_start[frame_offset as usize..];

    loop {
        let (end_current_frame, frame) = parse_frame(frames_start, netmsg_parse_mode, aux.clone())?;

        let is_next_section = matches!(frame.frame_data, FrameData::NextSection);

        frames_start = end_current_frame;
        frames.push(frame);

        if is_next_section {
            break;
        }
    }

    Ok((
        end_of_current_directory_entry,
        DirectoryEntry {
            type_,
            description: description.into(),
            flags,
            cd_track,
            track_time,
            frame_count,
            frame_offset,
            file_length,
            frames,
        },
    ))
}

pub fn parse_frame(
    i: &[u8],
    netmsg_parse_mode: MessageDataParseMode,
    aux: AuxRefCell,
) -> Result<Frame> {
    let source_len = aux.borrow().parsing_source_len;
    let (i, (type_, time, frame)) = (le_u8, le_f32, le_i32).parse(i)?;

    let (i, frame_data) = match type_ {
        2 => (i, FrameData::DemoStart),
        3 => map(parse_console_command, FrameData::ConsoleCommand).parse(i)?,
        4 => map(parse_client_data, FrameData::ClientData).parse(i)?,
        5 => (i, FrameData::NextSection),
        6 => map(parse_event, FrameData::Event).parse(i)?,
        7 => map(parse_weapon_animation, FrameData::WeaponAnimation).parse(i)?,
        8 => map(parse_sound, FrameData::Sound).parse(i)?,
        9 => map(parse_demo_buffer, FrameData::DemoBuffer).parse(i)?,
        rest => {
            let (i, res) = parse_network_messages(i, netmsg_parse_mode, aux)?;
            (
                i,
                FrameData::NetworkMessage(Box::new((
                    NetworkMessageType::try_from(rest).unwrap(),
                    res,
                ))),
            )
        }
    };

    if let Some(len) = source_len {
        crate::progress::report(len.saturating_sub(i.len()), len);
    }

    Ok((
        i,
        Frame {
            time,
            frame,
            frame_data,
        },
    ))
}

pub fn parse_console_command(i: &[u8]) -> Result<ConsoleCommand> {
    map(take(64usize), |command: &[u8]| ConsoleCommand {
        command: command.into(),
    })
    .parse(i)
}

pub fn parse_client_data(i: &[u8]) -> Result<ClientData> {
    map(
        (take_point_float, take_point_float, le_i32, le_f32),
        |(origin, viewangles, weapon_bits, fov)| ClientData {
            origin,
            viewangles,
            weapon_bits,
            fov,
        },
    )
    .parse(i)
}

pub fn parse_event(i: &[u8]) -> Result<Event> {
    map(
        (le_i32, le_i32, le_f32, parse_event_args),
        |(flags, index, delay, args)| Event {
            flags,
            index,
            delay,
            args,
        },
    )
    .parse(i)
}

pub fn parse_event_args(i: &[u8]) -> Result<EventArgs> {
    map(
        (
            le_i32,
            le_i32,
            take_point_float,
            take_point_float,
            take_point_float,
            le_i32,
            le_f32,
            le_f32,
            le_i32,
            le_i32,
            le_i32,
            le_i32,
        ),
        |(
            flags,
            entity_index,
            origin,
            angles,
            velocity,
            ducking,
            fparam1,
            fparam2,
            iparam1,
            iparam2,
            bparam1,
            bparam2,
        )| EventArgs {
            flags,
            entity_index,
            origin,
            angles,
            velocity,
            ducking,
            fparam1,
            fparam2,
            iparam1,
            iparam2,
            bparam1,
            bparam2,
        },
    )
    .parse(i)
}

pub fn parse_weapon_animation(i: &[u8]) -> Result<WeaponAnimation> {
    map((le_i32, le_i32), |(anim, body)| WeaponAnimation {
        anim,
        body,
    })
    .parse(i)
}

pub fn parse_sound(i: &[u8]) -> Result<Sound> {
    let (i, (channel, sample_length)) = (le_i32, le_u32).parse(i)?;

    // cannot return res directly because it is a closure and `channel` is outside of it

    #[allow(clippy::let_and_return)]
    let res = map(
        (take(sample_length), le_f32, le_f32, le_i32, le_i32),
        |(sample, attenuation, volume, flags, pitch): (&[u8], _, _, _, _)| Sound {
            channel,
            sample: sample.to_vec(),
            attenuation,
            volume,
            flags,
            pitch,
        },
    )
    .parse(i);

    res
}

pub fn parse_demo_buffer(i: &[u8]) -> Result<DemoBuffer> {
    let (i, buffer_length) = le_u32(i)?;

    map(take(buffer_length), |buffer: &[u8]| DemoBuffer {
        buffer: buffer.to_vec(),
    })
    .parse(i)
}

pub fn parse_network_messages(
    i: &[u8],
    netmsg_parse_mode: MessageDataParseMode,
    aux: AuxRefCell,
) -> Result<NetworkMessage> {
    let (i, (info, sequence_info, message_length)) =
        (parse_network_messages_info, parse_sequence_info, le_u32).parse(i)?;

    if message_length > 65536 {
        return nom_fail(format!("message length too long: {}", message_length));
    }

    // `message_length` is bounded above, but a truncated file can still leave
    // fewer bytes than it claims -- splitting on it unchecked panics. #225.
    if message_length as usize > i.len() {
        return nom_fail(format!(
            "message length {} runs past the end of the file ({} bytes left)",
            message_length,
            i.len()
        ));
    }
    let netmessage_data_chunk = &i[..message_length as usize];
    let the_rest = &i[message_length as usize..];
    // let (i, netmessage_data_chunk) = count(le_u8, message_length as usize)(i)?;

    let mut source_span = None;
    let messages = match netmsg_parse_mode {
        MessageDataParseMode::Parse => {
            // `i` starts at the payload and, under `parse_demo`, runs to the end
            // of the buffer, so its length says how far from that end the
            // payload begins.
            source_span = aux.borrow().parsing_source_len.and_then(|len| {
                let start = len.checked_sub(i.len())?;
                Some(start..start + message_length as usize)
            });

            // only parse the chunk
            // otherwise, it might spill outside, which it will
            let (_, netmessages) = parse_netmsg(netmessage_data_chunk, aux)?;

            MessageData::Parsed(netmessages)
        }
        MessageDataParseMode::Raw => MessageData::Raw(netmessage_data_chunk.to_vec()),
        MessageDataParseMode::None => MessageData::None,
    };

    Ok((
        the_rest,
        NetworkMessage {
            info,
            sequence_info,
            message_length,
            messages,
            source_span,
        },
    ))
}

pub fn parse_network_messages_info(i: &[u8]) -> Result<DemoInfo> {
    map(
        (
            le_f32,
            parse_refparams,
            parse_usercmd,
            parse_movevars,
            take_point_float,
            le_i32,
        ),
        |(timestamp, refparams, usercmd, movevars, view, viewmodel)| DemoInfo {
            timestamp,
            refparams,
            usercmd,
            movevars,
            view,
            viewmodel,
        },
    )
    .parse(i)
}

pub fn parse_refparams(i: &[u8]) -> Result<RefParams> {
    map(
        (
            (
                take_point_float,
                take_point_float,
                take_point_float,
                take_point_float,
                take_point_float,
            ),
            le_f32,
            le_f32,
            (le_i32, le_i32, le_i32, le_i32),
            le_i32,
            (take_point_float, take_point_float),
            (
                take_point_float,
                le_f32,
                take_point_float,
                le_i32,
                take_point_float,
                le_f32,
                take_point_float,
            ),
            (
                le_i32,
                le_i32,
                le_i32,
                le_i32,
                le_i32,
                le_i32,
                le_i32,
                le_i32,
                le_i32,
                count(le_i32, 4),
                le_i32,
                le_i32,
            ),
        ),
        |(
            (view_origin, view_angles, forward, right, up),
            frame_time,
            time,
            (intermission, paused, spectator, on_ground),
            water_level,
            (sim_vel, sim_org),
            (
                view_height,
                ideal_pitch,
                cl_viewangles,
                health,
                crosshair_angle,
                view_size,
                punch_angle,
            ),
            (
                max_clients,
                view_entity,
                player_num,
                max_entities,
                demo_playback,
                hardware,
                smoothing,
                ptr_cmd,
                ptr_move_vars,
                view_port,
                next_view,
                only_client_draw,
            ),
        )| RefParams {
            view_origin,
            view_angles,
            forward,
            right,
            up,
            frame_time,
            time,
            intermission,
            paused,
            spectator,
            on_ground,
            water_level,
            sim_vel,
            sim_org,
            view_height,
            ideal_pitch,
            cl_viewangles,
            health,
            crosshair_angle,
            view_size,
            punch_angle,
            max_clients,
            view_entity,
            player_num,
            max_entities,
            demo_playback,
            hardware,
            smoothing,
            ptr_cmd,
            ptr_move_vars,
            view_port,
            next_view,
            only_client_draw,
        },
    )
    .parse(i)
}

pub fn parse_usercmd(i: &[u8]) -> Result<UserCmd> {
    map(
        (
            le_i16,
            le_u8,
            le_u8,
            take_point_float,
            le_f32,
            le_f32,
            le_f32,
            le_i8,
            le_u8,
            le_u16,
            le_i8,
            le_i8,
            le_u8,
            le_u8,
            le_i32,
            take_point_float,
        ),
        |(
            lerp_msec,
            msec,
            unknown1,
            view_angles,
            forward_move,
            side_move,
            up_move,
            light_level,
            unknonwn2,
            buttons,
            impulse,
            weapon_select,
            unknown3,
            unknown4,
            impact_index,
            impact_position,
        )| UserCmd {
            lerp_msec,
            msec,
            unknown1,
            view_angles,
            forward_move,
            side_move,
            up_move,
            light_level,
            unknonwn2,
            buttons,
            impulse,
            weapon_select,
            unknown3,
            unknown4,
            impact_index,
            impact_position,
        },
    )
    .parse(i)
}

pub fn parse_movevars(i: &[u8]) -> Result<MoveVars> {
    map(
        (
            le_f32,
            (le_f32, le_f32, le_f32),
            (
                le_f32, le_f32, le_f32, le_f32, le_f32, le_f32, le_f32, le_f32, le_f32, le_f32,
            ),
            le_f32,
            le_f32,
            le_i32,
            take(32usize),
            (le_f32, le_f32),
            (take_point_float, take_point_float),
        ),
        |(
            gravity,
            (stopspeed, maxspeed, spectatormaxspeed),
            (
                accelerate,
                airaccelerate,
                wateraccelerate,
                friction,
                edgefriction,
                waterfriction,
                entgravity,
                bounce,
                stepsize,
                maxvelocity,
            ),
            zmax,
            wave_height,
            footsteps,
            sky_name,
            (rollangle, rollspeed),
            (skycolor, skyvec),
        ): (_, _, _, _, _, _, &[u8], _, _)| MoveVars {
            gravity,
            stopspeed,
            maxspeed,
            spectatormaxspeed,
            accelerate,
            airaccelerate,
            wateraccelerate,
            friction,
            edgefriction,
            waterfriction,
            entgravity,
            bounce,
            stepsize,
            maxvelocity,
            zmax,
            wave_height,
            footsteps,
            sky_name: sky_name.into(),
            rollangle,
            rollspeed,
            skycolor,
            skyvec,
        },
    )
    .parse(i)
}

pub fn parse_sequence_info(i: &[u8]) -> Result<SequenceInfo> {
    map(
        (le_i32, le_i32, le_i32, le_i32, le_i32, le_i32, le_i32),
        |(
            incoming_sequence,
            incoming_acknowledged,
            incoming_reliable_acknowledged,
            incoming_reliable_sequence,
            outgoing_sequence,
            reliable_sequence,
            last_reliable_sequence,
        )| SequenceInfo {
            incoming_sequence,
            incoming_acknowledged,
            incoming_reliable_acknowledged,
            incoming_reliable_sequence,
            outgoing_sequence,
            reliable_sequence,
            last_reliable_sequence,
        },
    )
    .parse(i)
}
