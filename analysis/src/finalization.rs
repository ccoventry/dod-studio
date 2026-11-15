//! The facts only the end of a demo settles: server name and address, whether
//! the recording began late or stopped early.

use crate::{AnalyzerEvent, AnalyzerState, Round};
use dem::types::{EngineMessage, FrameData};
use std::time::Duration;

pub fn use_time_left_updates(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    if let AnalyzerEvent::UserMessage(dod::UserMessage::TimeLeft(time_left)) = event {
        let duration = time_left.0;
        if state.first_time_left.is_none() {
            state.first_time_left = Some(duration);
        }
        state.last_time_left = Some(duration);
    }
}

fn is_close_to_match_end(duration: Duration) -> bool {
    let secs = duration.as_secs();
    let targets = [900u64, 1200, 1500, 1800, 2400, 2700];
    for &target in &targets {
        if secs >= target.saturating_sub(10) && secs <= target + 30 {
            return true;
        }
    }
    false
}

fn extract_ip_port(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let n = bytes.len();
    for i in 0..n {
        if bytes[i] == b':' {
            // Read digits to the right (port)
            let mut j = i + 1;
            while j < n && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let port_len = j - (i + 1);
            if port_len > 0 {
                // Read valid domain/ip characters to the left
                let mut k = i;
                while k > 0 {
                    let c = bytes[k - 1];
                    if c.is_ascii_alphanumeric() || c == b'.' || c == b'-' {
                        k -= 1;
                    } else {
                        break;
                    }
                }
                let host_str = &s[k..i];
                // Must contain at least one dot to be a valid domain or IP
                if host_str.contains('.') && !host_str.starts_with('.') && !host_str.ends_with('.')
                {
                    let port_str = &s[i + 1..j];
                    return Some(format!("{}:{}", host_str, port_str));
                }
            }
        }
    }
    None
}

pub fn use_general_finalization(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    if let AnalyzerEvent::EngineMessage(EngineMessage::SvcServerInfo(msg)) = event {
        let hostname = String::from_utf8_lossy(&msg.hostname)
            .trim_end_matches('\0')
            .to_string();
        state.server_name = Some(hostname.clone());
        if let Some(addr) = extract_ip_port(&hostname) {
            state.server_address = Some(addr);
        }
    }

    if let AnalyzerEvent::EngineMessage(EngineMessage::SvcStuffText(msg)) = event {
        let cmd = String::from_utf8_lossy(msg.command.as_slice());
        if let Some(addr) = extract_ip_port(&cmd) {
            state.server_address = Some(addr);
        }
    }

    if let AnalyzerEvent::Frame(frame) = event
        && let FrameData::ConsoleCommand(cmd) = &frame.frame_data
    {
        let cmd_str = String::from_utf8_lossy(cmd.command.as_slice());
        if let Some(addr) = extract_ip_port(&cmd_str) {
            state.server_address = Some(addr);
        }
    }

    use_time_left_updates(state, event);

    if let AnalyzerEvent::Finalization = event {
        if state.is_clan_match() {
            state.started_late = !state.match_start_witnessed;
        } else {
            state.started_late = state.players.iter().any(|p| p.has_pre_demo_activity);
        }

        let match_duration = if let Some(first_round) = state.rounds.first() {
            let start = match first_round {
                Round::Active { start_time, .. } => start_time.real_offset,
                Round::Completed { start_time, .. } => start_time.real_offset,
            };
            state.current_time.real_offset.saturating_sub(start)
        } else {
            Duration::ZERO
        };

        let is_natural_end = state.map_changed
            || state
                .last_time_left
                .is_some_and(|tl| tl <= Duration::from_secs(10))
            || is_close_to_match_end(match_duration);

        state.ended_early = if is_natural_end {
            false
        } else if let Some(last_round) = state.rounds.last() {
            match last_round {
                Round::Completed { winner_stats, .. } => winner_stats.is_none(),
                _ => true,
            }
        } else {
            false
        };
    }
}
