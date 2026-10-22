use std::{ffi::OsStr, fs::OpenOptions, io::Write, path::Path};

use crate::{
    byte_writer::ByteWriter,
    types::{Demo, FrameData, MessageData, NetworkMessageType},
};

impl Demo {
    pub fn write_to_file(&self, path: impl AsRef<OsStr> + AsRef<Path>) -> eyre::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;

        file.write_all(self.write_to_bytes().as_slice())?;
        file.flush()?;

        Ok(())
    }

    pub fn write_to_bytes(&self) -> Vec<u8> {
        // The check never fires, so `None` is unreachable here.
        match self.write_to_bytes_cancellable(&|| false) {
            Some(bytes) => bytes,
            None => unreachable!("a never-cancelling check reported cancellation"),
        }
    }

    /// `write_to_bytes`, abandonable partway through.
    ///
    /// Serialising a full-length GoldSrc demo is the single most expensive step
    /// in dod-studio' decal-flush pass — about 3 seconds for a 110MB, 730k-frame
    /// demo, roughly 70% of the whole pass — and it used to be one
    /// uninterruptible call. A user cancelling a capture batch had to wait it
    /// out, once per job still in flight. See dod-studio#193. (The flush now
    /// writes with `write_to_bytes_reusing_source_cancellable`, which copies
    /// the frames it did not edit instead of re-encoding them: ~3s down to
    /// ~80ms on that demo.)
    ///
    /// `should_cancel` is polled once every `CANCEL_CHECK_FRAMES` frames rather
    /// than per frame, which keeps an atomic load off a loop that runs
    /// hundreds of thousands of times while still bounding the wait to a few
    /// milliseconds of writing. Returns `None` if it gave up; the partial
    /// buffer is dropped rather than returned, so a cancelled write can never
    /// be mistaken for a complete demo.
    pub fn write_to_bytes_cancellable(&self, should_cancel: &dyn Fn() -> bool) -> Option<Vec<u8>> {
        self.write_frames(None, should_cancel)
    }

    /// `write_to_bytes_cancellable`, copying every network frame whose messages
    /// are untouched since the parse straight out of `source` instead of
    /// re-encoding them.
    ///
    /// Re-encoding is what makes the write slow: every message of every frame
    /// goes back through its encoder, delta packets included. A demo edited in
    /// a few thousand frames out of hundreds of thousands already has the
    /// encoding of all the rest sitting in the file it was parsed from.
    ///
    /// `source` must be the very buffer this demo was parsed from: the same
    /// allocation, not merely equal bytes. Anything else is ignored and every
    /// frame is re-encoded, exactly as `write_to_bytes_cancellable` does, so a
    /// wrong buffer costs speed, never correctness. A frame is only copied
    /// while its [`NetworkMessage::source_span`] survives, which is why edits
    /// must go through [`NetworkMessage::messages_mut`].
    ///
    /// [`NetworkMessage::source_span`]: crate::types::NetworkMessage::source_span
    /// [`NetworkMessage::messages_mut`]: crate::types::NetworkMessage::messages_mut
    pub fn write_to_bytes_reusing_source_cancellable(
        &self,
        source: &[u8],
        should_cancel: &dyn Fn() -> bool,
    ) -> Option<Vec<u8>> {
        let parsed_from = self._aux.as_ref().and_then(|aux| aux.borrow().parsed_from);
        let source =
            (parsed_from == Some((source.as_ptr() as usize, source.len()))).then_some(source);
        self.write_frames(source, should_cancel)
    }

    fn write_frames(
        &self,
        source: Option<&[u8]>,
        should_cancel: &dyn Fn() -> bool,
    ) -> Option<Vec<u8>> {
        /// Frames between cancellation polls. At roughly 4µs a frame this is a
        /// check every ~15ms of writing.
        const CANCEL_CHECK_FRAMES: usize = 4096;
        let mut frames_since_check = 0usize;

        // Copying most frames verbatim makes the output about the size of the
        // source, so reserve that up front rather than regrowing ~27 times.
        let mut writer = ByteWriter::with_capacity(source.map_or(0, <[u8]>::len));

        // Magic has 8 bytes in total
        writer.append_u8_slice("HLDEMO\x00\x00".as_bytes());

        // header
        writer.append_i32(self.header.demo_protocol);
        writer.append_i32(self.header.network_protocol);
        writer.append_u8_slice(self.header.map_name.padded(260).as_slice());
        writer.append_u8_slice(self.header.game_directory.padded(260).as_slice());
        writer.append_u32(self.header.map_checksum);

        // directory
        // Delay writing directory offset
        let directory_offset_pos = writer.get_offset();
        writer.append_i32(0i32);

        let mut entry_offsets: Vec<(usize, usize)> = vec![];

        for entry in &self.directory.entries {
            let mut has_written_next_section = false;

            let entry_offset_start = writer.get_offset();

            for frame in &entry.frames {
                frames_since_check += 1;
                if frames_since_check >= CANCEL_CHECK_FRAMES {
                    frames_since_check = 0;
                    if should_cancel() {
                        return None;
                    }
                }

                match frame.frame_data {
                    FrameData::DemoStart => writer.append_u8(2u8),
                    FrameData::ConsoleCommand(_) => writer.append_u8(3u8),
                    FrameData::ClientData(_) => writer.append_u8(4u8),
                    FrameData::NextSection => writer.append_u8(5u8),
                    FrameData::Event(_) => writer.append_u8(6u8),
                    FrameData::WeaponAnimation(_) => writer.append_u8(7u8),
                    FrameData::Sound(_) => writer.append_u8(8u8),
                    FrameData::DemoBuffer(_) => writer.append_u8(9u8),
                    FrameData::NetworkMessage(ref box_type) => match box_type.as_ref().0 {
                        NetworkMessageType::Start => writer.append_u8(0u8),
                        NetworkMessageType::Normal => writer.append_u8(1u8),
                        NetworkMessageType::Unknown(what) => writer.append_u8(what),
                    },
                }

                writer.append_f32(frame.time);
                writer.append_i32(frame.frame);

                // write the frame
                match &frame.frame_data {
                    FrameData::DemoStart => (),
                    FrameData::ConsoleCommand(frame) => {
                        writer.append_u8_slice(frame.command.padded(64).as_slice())
                    }
                    FrameData::ClientData(frame) => {
                        writer.append_f32_slice(frame.origin.as_slice());
                        writer.append_f32_slice(frame.viewangles.as_slice());
                        writer.append_i32(frame.weapon_bits);
                        writer.append_f32(frame.fov);
                    }
                    FrameData::NextSection => (),
                    FrameData::Event(frame) => {
                        writer.append_i32(frame.flags);
                        writer.append_i32(frame.index);
                        writer.append_f32(frame.delay);

                        writer.append_i32(frame.args.flags);
                        writer.append_i32(frame.args.entity_index);
                        writer.append_f32_slice(frame.args.origin.as_slice());
                        writer.append_f32_slice(frame.args.angles.as_slice());
                        writer.append_f32_slice(frame.args.velocity.as_slice());
                        writer.append_i32(frame.args.ducking);
                        writer.append_f32(frame.args.fparam1);
                        writer.append_f32(frame.args.fparam2);
                        writer.append_i32(frame.args.iparam1);
                        writer.append_i32(frame.args.iparam2);
                        writer.append_i32(frame.args.bparam1);
                        writer.append_i32(frame.args.bparam2);
                    }
                    FrameData::WeaponAnimation(frame) => {
                        writer.append_i32(frame.anim);
                        writer.append_i32(frame.body);
                    }
                    FrameData::Sound(frame) => {
                        writer.append_i32(frame.channel);
                        writer.append_i32(frame.sample.len() as i32);
                        writer.append_u8_slice(frame.sample.as_slice());
                        writer.append_f32(frame.attenuation);
                        writer.append_f32(frame.volume);
                        writer.append_i32(frame.flags);
                        writer.append_i32(frame.pitch);
                    }
                    FrameData::DemoBuffer(frame) => {
                        writer.append_i32(frame.buffer.len() as i32);
                        writer.append_u8_slice(frame.buffer.as_slice());
                    }
                    FrameData::NetworkMessage(ref box_type) => {
                        let data = &box_type.as_ref().1;

                        writer.append_f32(data.info.timestamp);
                        // ref_params
                        writer.append_f32_slice(data.info.refparams.view_origin.as_slice());
                        writer.append_f32_slice(data.info.refparams.view_angles.as_slice());
                        writer.append_f32_slice(data.info.refparams.forward.as_slice());
                        writer.append_f32_slice(data.info.refparams.right.as_slice());
                        writer.append_f32_slice(data.info.refparams.up.as_slice());
                        writer.append_f32(data.info.refparams.frame_time);
                        writer.append_f32(data.info.refparams.time);
                        writer.append_i32(data.info.refparams.intermission);
                        writer.append_i32(data.info.refparams.paused);
                        writer.append_i32(data.info.refparams.spectator);
                        writer.append_i32(data.info.refparams.on_ground);
                        writer.append_i32(data.info.refparams.water_level);
                        writer.append_f32_slice(data.info.refparams.sim_vel.as_slice());
                        writer.append_f32_slice(data.info.refparams.sim_org.as_slice());
                        writer.append_f32_slice(data.info.refparams.view_height.as_slice());
                        writer.append_f32(data.info.refparams.ideal_pitch);
                        writer.append_f32_slice(data.info.refparams.cl_viewangles.as_slice());
                        writer.append_i32(data.info.refparams.health);
                        writer.append_f32_slice(data.info.refparams.crosshair_angle.as_slice());
                        writer.append_f32(data.info.refparams.view_size);
                        writer.append_f32_slice(data.info.refparams.punch_angle.as_slice());
                        writer.append_i32(data.info.refparams.max_clients);
                        writer.append_i32(data.info.refparams.view_entity);
                        writer.append_i32(data.info.refparams.player_num);
                        writer.append_i32(data.info.refparams.max_entities);
                        writer.append_i32(data.info.refparams.demo_playback);
                        writer.append_i32(data.info.refparams.hardware);
                        writer.append_i32(data.info.refparams.smoothing);
                        writer.append_i32(data.info.refparams.ptr_cmd);
                        writer.append_i32(data.info.refparams.ptr_move_vars);
                        writer.append_i32_slice(data.info.refparams.view_port.as_slice());
                        writer.append_i32(data.info.refparams.next_view);
                        writer.append_i32(data.info.refparams.only_client_draw);
                        // usercmd
                        writer.append_i16(data.info.usercmd.lerp_msec);
                        writer.append_u8(data.info.usercmd.msec);
                        writer.append_u8(0u8); // unknown
                        writer.append_f32_slice(data.info.usercmd.view_angles.as_slice());
                        writer.append_f32(data.info.usercmd.forward_move);
                        writer.append_f32(data.info.usercmd.side_move);
                        writer.append_f32(data.info.usercmd.up_move);
                        writer.append_i8(data.info.usercmd.light_level);
                        writer.append_u8(0u8); // unknown
                        writer.append_u16(data.info.usercmd.buttons);
                        writer.append_i8(data.info.usercmd.impulse);
                        writer.append_i8(data.info.usercmd.weapon_select);
                        writer.append_u8(0u8); // unknown
                        writer.append_u8(0u8); // unknown
                        writer.append_i32(data.info.usercmd.impact_index);
                        writer.append_f32_slice(data.info.usercmd.impact_position.as_slice());
                        // movevars
                        writer.append_f32(data.info.movevars.gravity);
                        writer.append_f32(data.info.movevars.stopspeed);
                        writer.append_f32(data.info.movevars.maxspeed);
                        writer.append_f32(data.info.movevars.spectatormaxspeed);
                        writer.append_f32(data.info.movevars.accelerate);
                        writer.append_f32(data.info.movevars.airaccelerate);
                        writer.append_f32(data.info.movevars.wateraccelerate);
                        writer.append_f32(data.info.movevars.friction);
                        writer.append_f32(data.info.movevars.edgefriction);
                        writer.append_f32(data.info.movevars.waterfriction);
                        writer.append_f32(data.info.movevars.entgravity);
                        writer.append_f32(data.info.movevars.bounce);
                        writer.append_f32(data.info.movevars.stepsize);
                        writer.append_f32(data.info.movevars.maxvelocity);
                        writer.append_f32(data.info.movevars.zmax);
                        writer.append_f32(data.info.movevars.wave_height);
                        writer.append_i32(data.info.movevars.footsteps);
                        writer.append_u8_slice(data.info.movevars.sky_name.padded(32).as_slice());
                        writer.append_f32(data.info.movevars.rollangle);
                        writer.append_f32(data.info.movevars.rollspeed);
                        writer.append_f32(data.info.movevars.skycolor[0]);
                        writer.append_f32(data.info.movevars.skycolor[1]);
                        writer.append_f32(data.info.movevars.skycolor[2]);
                        writer.append_f32(data.info.movevars.skyvec[0]);
                        writer.append_f32(data.info.movevars.skyvec[1]);
                        writer.append_f32(data.info.movevars.skyvec[2]);
                        // still in info
                        writer.append_f32_slice(data.info.view.as_slice());
                        writer.append_i32(data.info.viewmodel);
                        // now other data
                        writer.append_i32(data.sequence_info.incoming_sequence);
                        writer.append_i32(data.sequence_info.incoming_acknowledged);
                        writer.append_i32(data.sequence_info.incoming_reliable_acknowledged);
                        writer.append_i32(data.sequence_info.incoming_reliable_sequence);
                        writer.append_i32(data.sequence_info.outgoing_sequence);
                        writer.append_i32(data.sequence_info.reliable_sequence);
                        writer.append_i32(data.sequence_info.last_reliable_sequence);

                        // An untouched frame's own bytes are already its
                        // encoding. Only ever `Some` for parsed messages whose
                        // span survived, read out of the buffer they came from.
                        let verbatim = match (&data.messages, &data.source_span, source) {
                            (MessageData::Parsed(_), Some(span), Some(source)) => {
                                source.get(span.clone())
                            }
                            _ => None,
                        };

                        // write the frame itself
                        match (&data.messages, verbatim) {
                            (_, Some(original)) => {
                                writer.append_u32(original.len() as u32);
                                writer.append_u8_slice(original);
                            }
                            (MessageData::Parsed(vec), None) => {
                                // delay writing message length
                                let start_offset_value = writer.get_offset();
                                writer.append_u32(0);

                                let start_length = writer.get_offset();

                                for message in vec {
                                    writer.append_u8_slice(
                                        message
                                            .write(self._aux.as_ref().unwrap().clone())
                                            .as_slice(),
                                    );
                                }

                                let end_length = writer.get_offset();

                                // this should be a function, wtf
                                writer.data.splice(
                                    start_offset_value..start_offset_value + 4,
                                    ((end_length - start_length) as u32).to_le_bytes(),
                                );
                            }
                            (MessageData::Raw(vec), None) => {
                                writer.append_i32(vec.len() as i32);
                                writer.append_u8_slice(vec.as_slice());
                            }
                            (MessageData::None, None) => {
                                // length
                                writer.append_i32(0);
                            }
                        }
                    }
                }

                if matches!(frame.frame_data, FrameData::NextSection) {
                    has_written_next_section = true;
                }
            }

            if !has_written_next_section {
                writer.append_u8(5u8);
                writer.append_f32(0.);
                writer.append_i32(0);
            }

            entry_offsets.push((entry_offset_start, writer.get_offset()));
        }

        // writing the directory entry at the end because now we have the offset
        let directory_offset = writer.get_offset();

        writer.append_i32(self.directory.entries.len() as i32);

        for (entry, (offset_start, offset_end)) in
            self.directory.entries.iter().zip(entry_offsets.iter())
        {
            writer.append_i32(entry.type_);
            writer.append_u8_slice(entry.description.padded(64).as_slice());
            writer.append_i32(entry.flags);
            writer.append_i32(entry.cd_track);
            writer.append_f32(entry.track_time);

            writer.append_i32(entry.frames.len() as i32);
            writer.append_i32(*offset_start as i32);
            writer.append_i32((offset_end - offset_start) as i32);
        }

        writer.data.splice(
            directory_offset_pos..directory_offset_pos + 4,
            (directory_offset as u32).to_le_bytes(),
        );

        Some(writer.data)
    }
}

#[cfg(test)]
mod reusing_source_tests {
    use crate::open_demo_from_bytes;
    use crate::types::{
        Aux, ByteString, Demo, Directory, DirectoryEntry, EngineMessage, Frame, FrameData, Header,
        MessageData, NetMessage, NetworkMessage, NetworkMessageType, SvcPrint, SvcTempEntity,
        TempEntity,
    };

    /// A small demo with real network frames: each carries a print, a world
    /// decal and a nop, so every frame has something that has to go back
    /// through an encoder if it is re-encoded rather than copied.
    fn built() -> Demo {
        // `DemoInfo` and `SequenceInfo` have no constructor; an all-zero one
        // parsed off the wire is as good as any and is what a writer would
        // have to reproduce anyway.
        let zeros = [0u8; 1024];
        let (_, info) = crate::demo_parser::parse_network_messages_info(&zeros).unwrap();
        let (_, sequence_info) = crate::demo_parser::parse_sequence_info(&zeros).unwrap();

        let network_frame = |i: usize| Frame {
            time: i as f32 * 0.01,
            frame: i as i32,
            frame_data: FrameData::NetworkMessage(Box::new((
                NetworkMessageType::Normal,
                NetworkMessage {
                    info: info.clone(),
                    sequence_info: sequence_info.clone(),
                    message_length: 0,
                    messages: MessageData::Parsed(vec![
                        // The terminator is part of the message on the wire.
                        NetMessage::EngineMessage(Box::new(EngineMessage::SvcPrint(SvcPrint {
                            message: ByteString::from(format!("frame {i}\n\0").as_str()),
                        }))),
                        NetMessage::EngineMessage(Box::new(EngineMessage::SvcTempEntity(
                            SvcTempEntity {
                                entity_type: 116,
                                entity: TempEntity::TeWorldDecal(vec![8, 0, 16, 0, 24, 0, i as u8]),
                            },
                        ))),
                        NetMessage::EngineMessage(Box::new(EngineMessage::SvcNop)),
                    ]),
                    source_span: None,
                },
            ))),
        };

        let mut frames = vec![Frame {
            time: 0.0,
            frame: 0,
            frame_data: FrameData::DemoStart,
        }];
        frames.extend((1..=40).map(network_frame));
        frames.push(Frame {
            time: 1.0,
            frame: 41,
            frame_data: FrameData::NextSection,
        });

        Demo {
            header: Header {
                magic: b"HLDEMO\x00\x00".to_vec(),
                demo_protocol: 5,
                network_protocol: 48,
                map_name: ByteString::from("dod_anzio"),
                game_directory: ByteString::from("dod"),
                map_checksum: 0,
                directory_offset: 0,
            },
            directory: Directory {
                entries: vec![DirectoryEntry {
                    type_: 1,
                    description: ByteString::from("Playback"),
                    flags: 0,
                    cd_track: -1,
                    track_time: 0.0,
                    frame_count: 0,
                    frame_offset: 0,
                    file_length: 0,
                    frames,
                }],
            },
            _aux: Some(Aux::new2()),
        }
    }

    /// The file `built()` writes, and the demo parsed back out of it.
    fn parsed() -> (Vec<u8>, Demo) {
        let bytes = built().write_to_bytes();
        let demo = open_demo_from_bytes(&bytes).unwrap();
        (bytes, demo)
    }

    fn network_message(demo: &mut Demo, frame: usize) -> &mut NetworkMessage {
        match &mut demo.directory.entries[0].frames[frame].frame_data {
            FrameData::NetworkMessage(b) => &mut b.1,
            _ => panic!("frame {frame} is not a network message"),
        }
    }

    fn reuse(demo: &Demo, source: &[u8]) -> Vec<u8> {
        demo.write_to_bytes_reusing_source_cancellable(source, &|| false)
            .expect("not cancelled")
    }

    #[test]
    fn every_parsed_network_frame_knows_where_it_came_from() {
        let (bytes, mut demo) = parsed();
        for i in 1..=40 {
            let nm = network_message(&mut demo, i);
            let span = nm
                .source_span
                .clone()
                .expect("a parsed frame records its span");
            assert_eq!(span.len(), nm.message_length as usize);
            assert!(span.end <= bytes.len());
        }
    }

    /// The guard on the whole idea: with nothing edited, copying every frame
    /// back out must give back exactly the file that was read.
    #[test]
    fn an_untouched_demo_is_written_back_byte_for_byte() {
        let (bytes, demo) = parsed();
        assert_eq!(reuse(&demo, &bytes), bytes);
    }

    #[test]
    fn an_edit_through_messages_mut_is_what_gets_written() {
        let (bytes, mut demo) = parsed();
        if let MessageData::Parsed(messages) = network_message(&mut demo, 7).messages_mut() {
            messages[1] = NetMessage::EngineMessage(Box::new(EngineMessage::SvcNop));
        }

        let reused = reuse(&demo, &bytes);
        assert_ne!(reused, bytes, "the edit was lost");
        assert_eq!(
            reused,
            demo.write_to_bytes(),
            "must match re-encoding every frame, the edited one included"
        );
    }

    /// Only the very buffer the demo was parsed from is trusted, not an equal
    /// copy of it. The edit below deliberately bypasses `messages_mut`, which
    /// is the one way to make the difference visible: against the original
    /// buffer its stale span would win, against anything else nothing is
    /// copied and the edit is re-encoded like every other frame.
    #[test]
    fn a_buffer_the_demo_was_not_parsed_from_is_not_trusted() {
        let (bytes, mut demo) = parsed();
        if let MessageData::Parsed(messages) = &mut network_message(&mut demo, 7).messages {
            messages.push(NetMessage::EngineMessage(Box::new(EngineMessage::SvcNop)));
        }

        let copy = bytes.clone();
        assert_eq!(reuse(&demo, &copy), demo.write_to_bytes());

        // And the hazard `messages_mut` exists for, pinned down so a change to
        // it is noticed: the same edit against the original buffer is lost.
        assert_eq!(reuse(&demo, &bytes), bytes);
    }

    #[test]
    fn a_built_demo_has_no_spans_and_is_simply_re_encoded() {
        let demo = built();
        let bytes = demo.write_to_bytes();
        assert_eq!(reuse(&demo, &bytes), bytes);
    }
}

#[cfg(test)]
mod cancellable_write_tests {
    use crate::types::{ByteString, Demo, Directory, DirectoryEntry, Frame, FrameData, Header};

    /// Enough frames to cross `CANCEL_CHECK_FRAMES` several times over, so a
    /// check that only ran per entry (there is one) would not be enough.
    const FRAMES: usize = 20_000;

    fn demo() -> Demo {
        let frames: Vec<Frame> = (0..FRAMES)
            .map(|i| Frame {
                time: i as f32,
                frame: i as i32,
                frame_data: FrameData::DemoStart,
            })
            .collect();

        Demo {
            header: Header {
                magic: b"HLDEMO  ".to_vec(),
                demo_protocol: 5,
                network_protocol: 48,
                map_name: ByteString::from("dod_anzio"),
                game_directory: ByteString::from("dod"),
                map_checksum: 0,
                directory_offset: 0,
            },
            directory: Directory {
                entries: vec![DirectoryEntry {
                    type_: 1,
                    description: ByteString::from("Playback"),
                    flags: 0,
                    cd_track: -1,
                    track_time: 0.0,
                    frame_count: FRAMES as i32,
                    frame_offset: 0,
                    file_length: 0,
                    frames,
                }],
            },
            _aux: None,
        }
    }

    #[test]
    fn a_check_that_never_fires_writes_the_whole_demo() {
        let demo = demo();
        let cancellable = demo
            .write_to_bytes_cancellable(&|| false)
            .expect("not cancelled");
        assert_eq!(
            cancellable,
            demo.write_to_bytes(),
            "must match the plain writer byte for byte"
        );
    }

    #[test]
    fn a_check_that_fires_abandons_the_write() {
        let demo = demo();
        assert!(
            demo.write_to_bytes_cancellable(&|| true).is_none(),
            "a cancelled write must yield nothing, never a truncated demo"
        );
    }

    /// The point of polling inside the frame loop rather than between entries:
    /// a real demo puts essentially all of its frames in one entry, so an
    /// entry-granular check would never fire mid-write.
    #[test]
    fn cancellation_is_noticed_partway_through_a_single_entry() {
        let demo = demo();
        let seen = std::cell::Cell::new(0usize);
        let result = demo.write_to_bytes_cancellable(&|| {
            seen.set(seen.get() + 1);
            seen.get() > 1
        });
        assert!(result.is_none());
        assert!(
            seen.get() > 1,
            "the check should have been polled more than once inside the one entry"
        );
    }
}
