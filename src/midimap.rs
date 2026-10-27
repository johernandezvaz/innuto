//! Controller mappings in the rekordbox `*.midi.csv` layout.
//!
//! Observed columns: `#name,function,type,input,deck1..deck4,output,deck1..deck4,option,comment`.
//! The input column is a four-hex-digit MIDI status+data1 pair (`900B`). A deck
//! column is either a channel offset added to the status (`0`, `1`) or a full
//! four-hex-digit code of its own (`9646`).

use std::collections::HashMap;

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub name: String,
    pub function: String,
    pub control_type: String,
    /// `None` for controls that are not deck-specific (crossfader, browser).
    pub deck: Option<usize>,
    pub status: u8,
    pub data1: u8,
}

#[derive(Debug, Default)]
pub struct Mapping {
    pub device: Option<String>,
    pub bindings: Vec<Binding>,
    index: HashMap<(u8, u8), usize>,
}

fn parse_code(s: &str) -> Option<(u8, u8)> {
    if s.len() != 4 {
        return None;
    }
    let v = u16::from_str_radix(s, 16).ok()?;
    Some(((v >> 8) as u8, v as u8))
}

impl Mapping {
    pub fn parse(text: &str) -> Result<Self> {
        let mut m = Mapping::default();
        for (line_no, line) in text.lines().enumerate() {
            let cols: Vec<&str> = line.split(',').map(str::trim).collect();
            let first = cols.first().copied().unwrap_or("");
            if first == "@file" {
                m.device = cols.get(2).map(|s| s.to_string());
                continue;
            }
            if first.is_empty() || first.starts_with('#') || cols.len() < 4 {
                continue;
            }
            let (name, function, control_type, input) = (cols[0], cols[1], cols[2], cols[3]);
            let decks: Vec<&str> = cols.iter().skip(4).take(4).copied().collect();
            let mut push = |deck: Option<usize>, (status, data1): (u8, u8)| {
                m.bindings.push(Binding {
                    name: name.to_string(),
                    function: function.to_string(),
                    control_type: control_type.to_string(),
                    deck,
                    status,
                    data1,
                });
            };
            let base = if input.is_empty() {
                None
            } else {
                Some(parse_code(input).with_context(|| {
                    format!("line {}: invalid MIDI code {input:?}", line_no + 1)
                })?)
            };
            let mut any_deck = false;
            for (d, val) in decks.iter().enumerate().filter(|(_, v)| !v.is_empty()) {
                any_deck = true;
                if let Some(code) = parse_code(val) {
                    push(Some(d), code);
                } else if let (Some((status, data1)), Ok(offset)) = (base, val.parse::<u8>()) {
                    push(Some(d), (status.wrapping_add(offset), data1));
                }
            }
            if !any_deck {
                if let Some(code) = base {
                    push(None, code);
                }
            }
        }
        for (i, b) in m.bindings.iter().enumerate() {
            m.index.entry((b.status, b.data1)).or_insert(i);
        }
        Ok(m)
    }

    /// Looks up the binding for an incoming message by status and data1.
    pub fn lookup(&self, status: u8, data1: u8) -> Option<&Binding> {
        self.index.get(&(status, data1)).map(|&i| &self.bindings[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "@file,1,DDJ-400\n\
#name,function,type,input,deck1,deck2,deck3,deck4,output,deck1,deck2,deck3,deck4,option,comment\n\
,,,,,,,,,,,,,,\n\
# Browser,,,,,,,,,,,,,,\n\
Load,Load,Button,,9646,9647,,,,,,,,,Load to Deck\n\
PlayPause,PlayPause,Button,900B,0,1,,,900B,0,1,,,Fast;Priority=50,Play/Pause\n\
CrossFader,CrossFader,KnobSliderHiRes,B61F,,,,,,,,,,Fast,Crossfader\n";

    #[test]
    fn parses_offsets_full_codes_and_global_controls() {
        let m = Mapping::parse(SAMPLE).unwrap();
        assert_eq!(m.device.as_deref(), Some("DDJ-400"));
        let play2 = m.lookup(0x91, 0x0B).unwrap();
        assert_eq!((play2.function.as_str(), play2.deck), ("PlayPause", Some(1)));
        let load1 = m.lookup(0x96, 0x46).unwrap();
        assert_eq!((load1.function.as_str(), load1.deck), ("Load", Some(0)));
        let xf = m.lookup(0xB6, 0x1F).unwrap();
        assert_eq!((xf.function.as_str(), xf.deck), ("CrossFader", None));
        assert!(m.lookup(0x92, 0x0B).is_none());
    }

    #[test]
    fn reports_invalid_codes_with_line_numbers() {
        let err = Mapping::parse("Cue,Cue,Button,ZZZZ,0,1").unwrap_err();
        assert!(err.to_string().contains("line 1"));
    }
}
