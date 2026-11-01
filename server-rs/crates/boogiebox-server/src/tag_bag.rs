//! Keeps every text tag a music file carries, so tags beyond the fixed scanner fields
//! (ReplayGain, MusicBrainz ids, multi-value credits) survive parsing.

use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const MAX_KEYS: usize = 256;
const MAX_VALUES_PER_KEY: usize = 32;
const MAX_VALUE_BYTES: usize = 4 * 1024;
const MAX_KEY_BYTES: usize = 255;
const MAX_APE_TAG_BYTES: u32 = 1024 * 1024;

/// Every text tag of one file, keyed by upper-case name, values in file order.
///
/// Keys are the format's own names: ID3 frame ids (`TIT2`), `TXXX:<DESCRIPTION>`, Vorbis and APEv2
/// field names (`REPLAYGAIN_TRACK_GAIN`), MP4 freeform atoms as `----:<MEAN>:<NAME>`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TagBag {
    values: HashMap<String, Vec<String>>,
}

impl TagBag {
    /// Adds one value; blank values, oversized values and anything past the size limits are dropped.
    pub fn push(&mut self, key: &str, value: &str) {
        let key = key.trim().to_uppercase();
        let value = value.trim_matches(|c: char| c == '\0' || c == '\u{feff}' || c.is_whitespace());
        if key.is_empty() || key.len() > MAX_KEY_BYTES || value.is_empty() {
            return;
        }
        if value.len() > MAX_VALUE_BYTES {
            return;
        }
        if !self.values.contains_key(&key) && self.values.len() >= MAX_KEYS {
            return;
        }
        let list = self.values.entry(key).or_default();
        if list.len() < MAX_VALUES_PER_KEY {
            list.push(value.to_owned());
        }
    }

    /// The first value stored under `key` (case-insensitive).
    pub fn first(&self, key: &str) -> Option<&str> {
        self.all(key).first().map(String::as_str)
    }

    /// Every value stored under `key` (case-insensitive), in file order.
    pub fn all(&self, key: &str) -> &[String] {
        self.values
            .get(&key.to_uppercase())
            .map_or(&[], Vec::as_slice)
    }

    /// Number of distinct keys.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// True when no tag was stored.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Appends `other`'s values after this bag's own.
    pub fn merge(&mut self, other: TagBag) {
        let mut entries: Vec<_> = other.values.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, values) in entries {
            for value in values {
                self.push(&key, &value);
            }
        }
    }
}

/// Reads the APEv2 tag at the end of a file (after the audio, or just before an ID3v1 block).
/// MP3Gain and older foobar2000 versions store ReplayGain there. Only UTF-8 text items are kept.
pub fn read_apev2(path: &Path) -> std::io::Result<TagBag> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    let mut bag = TagBag::default();
    for trailer in [0_u64, 128] {
        let Some(footer_at) = len.checked_sub(trailer + 32) else {
            continue;
        };
        if trailer == 128 {
            let mut id3v1 = [0_u8; 3];
            file.seek(SeekFrom::Start(len - 128))?;
            file.read_exact(&mut id3v1)?;
            if &id3v1 != b"TAG" {
                continue;
            }
        }
        let mut footer = [0_u8; 32];
        file.seek(SeekFrom::Start(footer_at))?;
        file.read_exact(&mut footer)?;
        if &footer[0..8] != b"APETAGEX" {
            continue;
        }
        let size = u32::from_le_bytes([footer[12], footer[13], footer[14], footer[15]]);
        let count = u32::from_le_bytes([footer[16], footer[17], footer[18], footer[19]]);
        if !(32..=MAX_APE_TAG_BYTES).contains(&size) || u64::from(size) > footer_at + 32 {
            return Ok(bag);
        }
        let items_len = (size - 32) as usize;
        let mut items = vec![0_u8; items_len];
        file.seek(SeekFrom::Start(footer_at + 32 - u64::from(size)))?;
        file.read_exact(&mut items)?;
        parse_ape_items(&items, count, &mut bag);
        return Ok(bag);
    }
    Ok(bag)
}

fn parse_ape_items(data: &[u8], count: u32, bag: &mut TagBag) {
    let mut offset = 0_usize;
    for _ in 0..count.min(MAX_KEYS as u32 * 2) {
        let Some(header) = data.get(offset..offset + 8) else {
            return;
        };
        let value_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let flags = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        offset += 8;
        let Some(key_len) = data
            .get(offset..)
            .and_then(|rest| rest.iter().position(|byte| *byte == 0))
        else {
            return;
        };
        let key = String::from_utf8_lossy(&data[offset..offset + key_len]).into_owned();
        offset += key_len + 1;
        let Some(value) = data.get(offset..offset.saturating_add(value_len)) else {
            return;
        };
        offset += value_len;
        // Bits 1-2 give the item type; 0 is UTF-8 text (binary and external links are skipped).
        if (flags >> 1) & 0b11 != 0 {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(value) {
            for part in text.split('\0') {
                bag.push(&key, part);
            }
        }
    }
}

/// ReplayGain values from the tags, in any of the places taggers put them: Vorbis comments and
/// APEv2 (`REPLAYGAIN_*`), ID3 `TXXX:REPLAYGAIN_*`, MP4 `----:com.apple.iTunes:replaygain_*`, and Opus
/// `R128_*_GAIN` (Q7.8 relative to −23 LUFS). `None` when there is no usable track gain.
pub fn loudness_tags(bag: &TagBag) -> Option<boogiebox_db::loudness::LoudnessTags> {
    let find = |name: &str| {
        [
            name.to_owned(),
            format!("TXXX:{name}"),
            format!("----:COM.APPLE.ITUNES:{name}"),
        ]
        .iter()
        .find_map(|key| bag.first(key).map(str::to_owned))
    };
    let gain = |name: &str| find(name).as_deref().and_then(parse_gain_db);
    let peak = |name: &str| find(name).as_deref().and_then(parse_peak);
    let r128 = |name: &str| bag.first(name).and_then(parse_r128_db);
    let tags = boogiebox_db::loudness::LoudnessTags {
        track_gain_db: gain("REPLAYGAIN_TRACK_GAIN").or_else(|| r128("R128_TRACK_GAIN")),
        track_peak: peak("REPLAYGAIN_TRACK_PEAK"),
        album_gain_db: gain("REPLAYGAIN_ALBUM_GAIN").or_else(|| r128("R128_ALBUM_GAIN")),
        album_peak: peak("REPLAYGAIN_ALBUM_PEAK"),
    };
    tags.track_gain_db.is_some().then_some(tags)
}

/// Parses `"-7.32 dB"`, `"-7.32"`, `"+1.5 dB"` or `"-7,32 dB"`; values outside ±30 dB are rejected.
fn parse_gain_db(value: &str) -> Option<f64> {
    let value = value.trim();
    let number = value
        .strip_suffix("dB")
        .or_else(|| value.strip_suffix("db"))
        .or_else(|| value.strip_suffix("DB"))
        .unwrap_or(value)
        .trim()
        .replace(',', ".");
    let gain: f64 = number.strip_prefix('+').unwrap_or(&number).parse().ok()?;
    (gain.is_finite() && gain.abs() <= 30.0).then_some(gain)
}

/// A linear sample peak; zero, negative or implausibly large peaks are rejected.
fn parse_peak(value: &str) -> Option<f64> {
    let peak: f64 = value.trim().replace(',', ".").parse().ok()?;
    (peak.is_finite() && peak > 0.0 && peak <= 10.0).then_some(peak)
}

/// Opus R128 gain: a Q7.8 integer relative to −23 LUFS, moved to the −18 LUFS reference.
fn parse_r128_db(value: &str) -> Option<f64> {
    let q78: i32 = value.trim().parse().ok()?;
    let gain = f64::from(q78) / 256.0 + 5.0;
    (gain.abs() <= 30.0).then_some(gain)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn ape_item(key: &str, value: &[u8], flags: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.push(0);
        out.extend_from_slice(value);
        out
    }

    pub(crate) fn apev2_tag(items: &[(&str, &str)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (key, value) in items {
            body.extend(ape_item(key, value.as_bytes(), 0));
        }
        body.extend(ape_item("Cover Art (Front)", &[1, 2, 3], 0b10));
        let mut footer = Vec::new();
        footer.extend_from_slice(b"APETAGEX");
        footer.extend_from_slice(&2000_u32.to_le_bytes());
        footer.extend_from_slice(&((body.len() + 32) as u32).to_le_bytes());
        footer.extend_from_slice(&((items.len() + 1) as u32).to_le_bytes());
        footer.extend_from_slice(&0_u32.to_le_bytes());
        footer.extend_from_slice(&[0; 8]);
        body.extend(footer);
        body
    }

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("boogiebox-tagbag-{name}-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("track.mp3");
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn keeps_every_value_in_order_and_matches_keys_case_insensitively() {
        let mut bag = TagBag::default();
        bag.push("artist", "Simon");
        bag.push("ARTIST", " Garfunkel ");
        bag.push("Artist", "  ");
        bag.push("TXXX:replaygain_track_gain", "-7.32 dB\0");
        assert_eq!(bag.all("Artist"), ["Simon", "Garfunkel"]);
        assert_eq!(bag.first("artist"), Some("Simon"));
        assert_eq!(bag.first("TXXX:REPLAYGAIN_TRACK_GAIN"), Some("-7.32 dB"));
        assert_eq!(bag.first("missing"), None);
        assert_eq!(bag.len(), 2);
    }

    #[test]
    fn limits_keys_values_and_sizes() {
        let mut bag = TagBag::default();
        for i in 0..300 {
            bag.push(&format!("KEY{i}"), "x");
        }
        assert_eq!(bag.len(), MAX_KEYS);
        for i in 0..40 {
            bag.push("KEY0", &i.to_string());
        }
        assert_eq!(bag.all("KEY0").len(), MAX_VALUES_PER_KEY);
        bag.push("KEY1", &"y".repeat(10 * 1024));
        assert_eq!(bag.all("KEY1"), ["x"]);
        let mut empty = TagBag::default();
        empty.push(&"K".repeat(300), "v");
        assert!(empty.is_empty());
    }

    #[test]
    fn merge_appends_after_own_values() {
        let mut a = TagBag::default();
        a.push("GENRE", "Jazz");
        let mut b = TagBag::default();
        b.push("GENRE", "Blues");
        b.push("MOOD", "Calm");
        a.merge(b);
        assert_eq!(a.all("GENRE"), ["Jazz", "Blues"]);
        assert_eq!(a.first("MOOD"), Some("Calm"));
    }

    #[test]
    fn reads_apev2_at_the_end_and_before_an_id3v1_block() {
        let mut audio = vec![0xff_u8, 0xfb, 0x90, 0x00];
        audio.extend(vec![0; 400]);
        let tag = apev2_tag(&[("REPLAYGAIN_TRACK_GAIN", "-6.20 dB"), ("Artist", "A\0B")]);

        let mut plain = audio.clone();
        plain.extend(&tag);
        let bag = read_apev2(&temp_file("plain", &plain)).unwrap();
        assert_eq!(bag.first("replaygain_track_gain"), Some("-6.20 dB"));
        assert_eq!(bag.all("ARTIST"), ["A", "B"]);
        assert!(bag.all("COVER ART (FRONT)").is_empty());

        let mut with_v1 = audio.clone();
        with_v1.extend(&tag);
        let mut id3v1 = b"TAG".to_vec();
        id3v1.resize(128, 0);
        with_v1.extend(id3v1);
        let bag = read_apev2(&temp_file("v1", &with_v1)).unwrap();
        assert_eq!(bag.first("REPLAYGAIN_TRACK_GAIN"), Some("-6.20 dB"));

        assert!(read_apev2(&temp_file("none", &audio)).unwrap().is_empty());
        assert!(read_apev2(&temp_file("tiny", b"ab")).unwrap().is_empty());
    }

    #[test]
    fn ignores_a_broken_apev2_footer() {
        let mut bytes = vec![0_u8; 64];
        let mut tag = apev2_tag(&[("REPLAYGAIN_TRACK_GAIN", "-6.20 dB")]);
        let footer = tag.len() - 32;
        tag[footer + 12..footer + 16].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend(tag);
        assert!(read_apev2(&temp_file("broken", &bytes)).unwrap().is_empty());
    }

    fn bag(pairs: &[(&str, &str)]) -> TagBag {
        let mut bag = TagBag::default();
        for (key, value) in pairs {
            bag.push(key, value);
        }
        bag
    }

    #[test]
    fn reads_replaygain_from_every_tag_family() {
        let vorbis = loudness_tags(&bag(&[
            ("REPLAYGAIN_TRACK_GAIN", "-7.32 dB"),
            ("REPLAYGAIN_TRACK_PEAK", "0.988"),
            ("REPLAYGAIN_ALBUM_GAIN", "+1.5 dB"),
            ("REPLAYGAIN_ALBUM_PEAK", "1,02"),
        ]))
        .unwrap();
        assert_eq!(vorbis.track_gain_db, Some(-7.32));
        assert_eq!(vorbis.track_peak, Some(0.988));
        assert_eq!(vorbis.album_gain_db, Some(1.5));
        assert_eq!(vorbis.album_peak, Some(1.02));

        let id3 = loudness_tags(&bag(&[("TXXX:replaygain_track_gain", "-6,5 db")])).unwrap();
        assert_eq!(id3.track_gain_db, Some(-6.5));
        assert_eq!(id3.album_gain_db, None);

        let mp4 = loudness_tags(&bag(&[(
            "----:com.apple.iTunes:replaygain_track_gain",
            "-3",
        )]))
        .unwrap();
        assert_eq!(mp4.track_gain_db, Some(-3.0));

        let opus = loudness_tags(&bag(&[
            ("R128_TRACK_GAIN", "-1280"),
            ("R128_ALBUM_GAIN", "256"),
        ]))
        .unwrap();
        assert_eq!(opus.track_gain_db, Some(0.0)); // -5 dB vs -23 LUFS is 0 dB vs -18 LUFS
        assert_eq!(opus.album_gain_db, Some(6.0));
    }

    #[test]
    fn rejects_unusable_replaygain_values() {
        assert_eq!(
            loudness_tags(&bag(&[("REPLAYGAIN_TRACK_GAIN", "loud")])),
            None
        );
        assert_eq!(
            loudness_tags(&bag(&[("REPLAYGAIN_TRACK_GAIN", "-45 dB")])),
            None
        );
        assert_eq!(
            loudness_tags(&bag(&[("REPLAYGAIN_TRACK_GAIN", "NaN")])),
            None
        );
        assert_eq!(
            loudness_tags(&bag(&[("REPLAYGAIN_ALBUM_GAIN", "-3 dB")])),
            None,
            "album only"
        );
        assert_eq!(loudness_tags(&bag(&[("R128_TRACK_GAIN", "1.5")])), None);
        let bad_peak = loudness_tags(&bag(&[
            ("REPLAYGAIN_TRACK_GAIN", "-2"),
            ("REPLAYGAIN_TRACK_PEAK", "0"),
        ]))
        .unwrap();
        assert_eq!(bad_peak.track_peak, None);
        assert_eq!(loudness_tags(&TagBag::default()), None);
    }
}
