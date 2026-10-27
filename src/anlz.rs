//! Reader and writer for Pioneer-style `ANLZ*.DAT/.EXT` analysis files.
//!
//! Layout as observed in rekordbox 7.2.18 output (see EVIDENCE.md): a big-endian
//! `PMAI` file header (28 bytes) followed by tagged sections, each starting with
//! a four-byte tag, a header length, and a total section length. Innuto decodes
//! path, beat grid, and waveform preview sections; every other tag is retained
//! as an opaque section so nothing is silently dropped.

use anyhow::{bail, ensure, Result};

use crate::analysis::Beat;

const FILE_HEADER_LEN: u32 = 28;

#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    /// `PPTH`: the track path the analysis belongs to.
    Path(String),
    /// `PQTZ`: beat grid with tempo in hundredths of a BPM per beat.
    BeatGrid(Vec<GridBeat>),
    /// `PWAV`: one byte per column (low 5 bits height, high 3 bits brightness).
    WaveformPreview(Vec<u8>),
    Other { tag: String, header_len: u32, len: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridBeat {
    pub bar_position: u16,
    pub tempo_centi_bpm: u16,
    pub time_ms: u32,
}

fn be32(b: &[u8], o: usize) -> Result<u32> {
    ensure!(o + 4 <= b.len(), "truncated at offset {o}");
    Ok(u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]))
}

fn be16(b: &[u8], o: usize) -> Result<u16> {
    ensure!(o + 2 <= b.len(), "truncated at offset {o}");
    Ok(u16::from_be_bytes([b[o], b[o + 1]]))
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Section>> {
    ensure!(bytes.len() >= FILE_HEADER_LEN as usize, "file shorter than PMAI header");
    if &bytes[0..4] != b"PMAI" {
        bail!("not an ANLZ file (missing PMAI magic)");
    }
    let header_len = be32(bytes, 4)? as usize;
    let total = be32(bytes, 8)? as usize;
    ensure!(total <= bytes.len(), "declared length {total} exceeds file size {}", bytes.len());

    let mut sections = Vec::new();
    let mut o = header_len;
    while o + 12 <= total {
        let tag = String::from_utf8_lossy(&bytes[o..o + 4]).into_owned();
        let hlen = be32(bytes, o + 4)?;
        let len = be32(bytes, o + 8)?;
        ensure!(len >= 12 && hlen <= len, "{tag} at {o}: invalid lengths {hlen}/{len}");
        let end = o + len as usize;
        ensure!(end <= total, "{tag} at {o} overruns file");
        let body = &bytes[o..end];
        sections.push(match tag.as_str() {
            "PPTH" => {
                let n = be32(body, 12)? as usize;
                let start = hlen as usize;
                ensure!(start + n <= body.len(), "PPTH path overruns section");
                let units: Vec<u16> = body[start..start + n]
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .take_while(|&u| u != 0)
                    .collect();
                Section::Path(String::from_utf16_lossy(&units))
            }
            "PQTZ" => {
                let count = be32(body, 20)? as usize;
                let start = hlen as usize;
                ensure!(start + count * 8 <= body.len(), "PQTZ beats overrun section");
                let beats = (0..count)
                    .map(|i| {
                        let e = start + i * 8;
                        Ok(GridBeat {
                            bar_position: be16(body, e)?,
                            tempo_centi_bpm: be16(body, e + 2)?,
                            time_ms: be32(body, e + 4)?,
                        })
                    })
                    .collect::<Result<_>>()?;
                Section::BeatGrid(beats)
            }
            "PWAV" => {
                let count = be32(body, 12)? as usize;
                let start = hlen as usize;
                ensure!(start + count <= body.len(), "PWAV data overruns section");
                Section::WaveformPreview(body[start..start + count].to_vec())
            }
            _ => Section::Other { tag, header_len: hlen, len },
        });
        o = end;
    }
    Ok(sections)
}

fn section(tag: &[u8; 4], header: &[u8], data: &[u8]) -> Vec<u8> {
    let hlen = 12 + header.len() as u32;
    let len = hlen + data.len() as u32;
    let mut v = Vec::with_capacity(len as usize);
    v.extend_from_slice(tag);
    v.extend_from_slice(&hlen.to_be_bytes());
    v.extend_from_slice(&len.to_be_bytes());
    v.extend_from_slice(header);
    v.extend_from_slice(data);
    v
}

/// Writes a `.DAT` with path, beat grid, and waveform preview sections.
pub fn write_dat(path: &str, bpm: f64, beats: &[Beat], preview: &[u8]) -> Vec<u8> {
    let mut path_data: Vec<u8> = path.encode_utf16().chain([0]).flat_map(u16::to_be_bytes).collect();
    if path_data.is_empty() {
        path_data.extend_from_slice(&[0, 0]);
    }
    let ppth = section(b"PPTH", &(path_data.len() as u32).to_be_bytes(), &path_data);

    let tempo = (bpm * 100.0).round() as u16;
    let mut grid_header = Vec::new();
    grid_header.extend_from_slice(&0u32.to_be_bytes());
    grid_header.extend_from_slice(&0x0008_0000u32.to_be_bytes());
    grid_header.extend_from_slice(&(beats.len() as u32).to_be_bytes());
    let grid_data: Vec<u8> = beats
        .iter()
        .flat_map(|b| {
            let mut e = Vec::with_capacity(8);
            e.extend_from_slice(&b.bar_position.to_be_bytes());
            e.extend_from_slice(&tempo.to_be_bytes());
            e.extend_from_slice(&b.time_ms.to_be_bytes());
            e
        })
        .collect();
    let pqtz = section(b"PQTZ", &grid_header, &grid_data);

    let mut wav_header = Vec::new();
    wav_header.extend_from_slice(&(preview.len() as u32).to_be_bytes());
    wav_header.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    let pwav = section(b"PWAV", &wav_header, preview);

    let total = FILE_HEADER_LEN as usize + ppth.len() + pqtz.len() + pwav.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"PMAI");
    out.extend_from_slice(&FILE_HEADER_LEN.to_be_bytes());
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.resize(FILE_HEADER_LEN as usize, 0);
    out.extend(ppth);
    out.extend(pqtz);
    out.extend(pwav);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_written_sections() {
        let beats = vec![
            Beat { bar_position: 1, time_ms: 226 },
            Beat { bar_position: 2, time_ms: 702 },
        ];
        let preview: Vec<u8> = (0..400).map(|i| (i % 32) as u8 | 0x40).collect();
        let bytes = write_dat("/Music/track.mp3", 126.0, &beats, &preview);
        let sections = parse(&bytes).unwrap();
        assert_eq!(sections[0], Section::Path("/Music/track.mp3".into()));
        assert_eq!(
            sections[1],
            Section::BeatGrid(vec![
                GridBeat { bar_position: 1, tempo_centi_bpm: 12600, time_ms: 226 },
                GridBeat { bar_position: 2, tempo_centi_bpm: 12600, time_ms: 702 },
            ])
        );
        assert_eq!(sections[2], Section::WaveformPreview(preview));
    }

    #[test]
    fn written_header_sizes_match_observed_layout() {
        // Observed: PPTH header 16, PQTZ header 24, PWAV header 20 (EVIDENCE.md).
        let bytes = write_dat("a", 120.0, &[], &[0; 400]);
        let mut o = 28;
        for (tag, hlen) in [("PPTH", 16), ("PQTZ", 24), ("PWAV", 20)] {
            assert_eq!(&bytes[o..o + 4], tag.as_bytes());
            assert_eq!(be32(&bytes, o + 4).unwrap(), hlen);
            o += be32(&bytes, o + 8).unwrap() as usize;
        }
        assert_eq!(o, bytes.len());
    }

    #[test]
    fn rejects_truncated_and_foreign_files() {
        assert!(parse(b"RIFF0000000000000000000000000000").is_err());
        let mut bytes = write_dat("a", 120.0, &[], &[0; 10]);
        bytes.truncate(bytes.len() - 4);
        assert!(parse(&bytes).is_err());
    }
}
