//! Encoder delay and padding of an MP4/M4A audio track, for gapless AAC. symphonia reads the edit list but doesn't
//! apply it, so the decoder trims by what this finds: the iTunes `iTunSMPB` tag first, else the audio track's edit
//! list (what FFmpeg writes).

use std::io::{self, Read, Seek, SeekFrom};

/// Largest `moov` box read (bigger ones are left untrimmed).
const MAX_MOOV: u64 = 16 << 20;
/// Top-level boxes looked at before giving up on finding `moov`.
const MAX_TOP_BOXES: usize = 64;

/// Where an MP4 audio track's real sound starts and how long it lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gapless {
    /// `iTunSMPB`: frames at the stream's own rate.
    Smpb {
        /// Priming frames to drop.
        delay: u64,
        /// Frames of real sound (0 = not given).
        valid: u64,
    },
    /// The audio track's first edit: `media_time` in the track's timescale, `duration` in the movie's.
    EditList {
        /// Where the sound starts in the track's timescale.
        media_time: u64,
        /// The track's (media) timescale.
        media_scale: u32,
        /// How long the sound lasts in the movie timescale (0 = to the end).
        duration: u64,
        /// The movie timescale.
        movie_scale: u32,
    },
}

impl Gapless {
    /// Priming frames and frames of real sound at `rate`.
    pub fn frames(&self, rate: u32) -> (u64, Option<u64>) {
        let rate = u128::from(rate);
        let rescale = |v: u64, scale: u32| -> u64 {
            let scale = u128::from(scale.max(1));
            ((u128::from(v) * rate + scale / 2) / scale) as u64
        };
        match *self {
            Gapless::Smpb { delay, valid } => (delay, (valid > 0).then_some(valid)),
            Gapless::EditList {
                media_time,
                media_scale,
                duration,
                movie_scale,
            } => (
                rescale(media_time, media_scale),
                (duration > 0).then(|| rescale(duration, movie_scale)),
            ),
        }
    }
}

/// Reads the gapless info of an MP4 file (`None` for other files, or when it has none). Leaves `r` anywhere.
pub fn read<R: Read + Seek>(r: &mut R) -> io::Result<Option<Gapless>> {
    r.seek(SeekFrom::Start(0))?;
    let mut pos = 0u64;
    for i in 0..MAX_TOP_BOXES {
        let Some((kind, header, size)) = read_header(r)? else {
            return Ok(None);
        };
        if i == 0 && &kind != b"ftyp" {
            return Ok(None);
        }
        if &kind == b"moov" {
            let body = size.map(|s| s.saturating_sub(header));
            let Some(len) = body.filter(|l| *l <= MAX_MOOV) else {
                return Ok(None);
            };
            let mut moov = vec![0u8; len as usize];
            r.read_exact(&mut moov)?;
            return Ok(parse_moov(&moov));
        }
        let Some(size) = size else { return Ok(None) };
        pos += size;
        r.seek(SeekFrom::Start(pos))?;
    }
    Ok(None)
}

/// A box header: type, header length and full size (`None` = to the end of the file).
type Header = ([u8; 4], u64, Option<u64>);

fn read_header<R: Read>(r: &mut R) -> io::Result<Option<Header>> {
    let mut h = [0u8; 8];
    match r.read_exact(&mut h) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let size = u32::from_be_bytes([h[0], h[1], h[2], h[3]]);
    let kind = [h[4], h[5], h[6], h[7]];
    Ok(Some(match size {
        0 => (kind, 8, None),
        1 => {
            let mut big = [0u8; 8];
            r.read_exact(&mut big)?;
            let size = u64::from_be_bytes(big);
            if size < 16 {
                return Ok(None);
            }
            (kind, 16, Some(size))
        }
        s if s < 8 => return Ok(None),
        s => (kind, 8, Some(u64::from(s))),
    }))
}

/// The child boxes of `data`, as (type, body).
fn boxes(data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut rest = data;
    std::iter::from_fn(move || {
        if rest.len() < 8 {
            return None;
        }
        let size = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let kind = [rest[4], rest[5], rest[6], rest[7]];
        let (header, size) = match size {
            0 => (8, rest.len()),
            1 => {
                let big = rest.get(8..16)?;
                (
                    16,
                    usize::try_from(u64::from_be_bytes(big.try_into().ok()?)).ok()?,
                )
            }
            s => (8, s),
        };
        if size < header || size > rest.len() {
            return None;
        }
        let body = &rest[header..size];
        rest = &rest[size..];
        Some((kind, body))
    })
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(data).find(|(k, _)| k == kind).map(|(_, b)| b)
}

fn be_u32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn be_u64(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

/// Timescale of an `mvhd` or `mdhd` body.
fn timescale(full_box: &[u8]) -> Option<u32> {
    match full_box.first()? {
        0 => be_u32(full_box, 12),
        1 => be_u32(full_box, 20),
        _ => None,
    }
}

fn parse_moov(moov: &[u8]) -> Option<Gapless> {
    if let Some(smpb) = itunsmpb(moov) {
        return Some(smpb);
    }
    let movie_scale = child(moov, b"mvhd").and_then(timescale)?;
    let trak = boxes(moov)
        .filter(|(k, _)| k == b"trak")
        .find_map(|(_, trak)| {
            let mdia = child(trak, b"mdia")?;
            let handler = child(mdia, b"hdlr")?.get(8..12)?;
            (handler == b"soun").then_some((trak, mdia))
        })?;
    let (trak, mdia) = trak;
    let media_scale = child(mdia, b"mdhd").and_then(timescale)?;
    let elst = child(child(trak, b"edts")?, b"elst")?;
    let version = *elst.first()?;
    let count = be_u32(elst, 4)? as usize;
    let entry = if version == 1 { 20 } else { 12 };
    (0..count.min(16)).find_map(|i| {
        let at = 8 + i * entry;
        let (duration, media_time) = if version == 1 {
            (be_u64(elst, at)?, be_u64(elst, at + 8)? as i64)
        } else {
            (
                u64::from(be_u32(elst, at)?),
                i64::from(be_u32(elst, at + 4)? as i32),
            )
        };
        // -1 is an empty edit (a pause before the track); the first real one says where the sound starts.
        (media_time >= 0).then_some(Gapless::EditList {
            media_time: media_time as u64,
            media_scale,
            duration,
            movie_scale,
        })
    })
}

/// `moov › udta › meta › ilst › ---- (name iTunSMPB)`: " 00000000 <delay> <padding> <valid frames> …" in hex.
fn itunsmpb(moov: &[u8]) -> Option<Gapless> {
    let meta = child(child(moov, b"udta")?, b"meta")?;
    // ISO `meta` is a full box (4 bytes version/flags); QuickTime's has none.
    let meta = if meta.get(4..8) == Some(b"hdlr") {
        meta
    } else {
        meta.get(4..)?
    };
    let ilst = child(meta, b"ilst")?;
    boxes(ilst)
        .filter(|(k, _)| k == b"----")
        .find_map(|(_, tag)| {
            let name = child(tag, b"name")?.get(4..)?;
            if name != b"iTunSMPB" {
                return None;
            }
            let text = std::str::from_utf8(child(tag, b"data")?.get(8..)?).ok()?;
            let mut fields = text.split_whitespace().map(|f| u64::from_str_radix(f, 16));
            let _zero = fields.next()?.ok()?;
            let delay = fields.next()?.ok()?;
            let _padding = fields.next()?.ok()?;
            let valid = fields.next()?.ok()?;
            Some(Gapless::Smpb { delay, valid })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(body);
        v
    }

    fn full(version: u8, rest: &[u8]) -> Vec<u8> {
        let mut v = vec![version, 0, 0, 0];
        v.extend_from_slice(rest);
        v
    }

    fn header_v0(scale: u32) -> Vec<u8> {
        let mut rest = vec![0u8; 8];
        rest.extend_from_slice(&scale.to_be_bytes());
        rest.extend_from_slice(&[0u8; 4]);
        full(0, &rest)
    }

    fn sound_trak(edit: Option<Vec<u8>>) -> Vec<u8> {
        let mut hdlr = vec![0u8; 4];
        hdlr.extend_from_slice(b"soun");
        hdlr.extend_from_slice(&[0u8; 12]);
        let mdia = [
            mp4_box(b"mdhd", &header_v0(44_100)),
            mp4_box(b"hdlr", &full(0, &hdlr)),
        ]
        .concat();
        let mut trak = Vec::new();
        if let Some(elst) = edit {
            trak.extend(mp4_box(b"edts", &mp4_box(b"elst", &elst)));
        }
        trak.extend(mp4_box(b"mdia", &mdia));
        mp4_box(b"trak", &trak)
    }

    fn elst_v0(entries: &[(u32, i32)]) -> Vec<u8> {
        let mut rest = (entries.len() as u32).to_be_bytes().to_vec();
        for (duration, media_time) in entries {
            rest.extend_from_slice(&duration.to_be_bytes());
            rest.extend_from_slice(&media_time.to_be_bytes());
            rest.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        }
        full(0, &rest)
    }

    fn file(moov_children: &[Vec<u8>], moov_last: bool) -> Vec<u8> {
        let ftyp = mp4_box(b"ftyp", b"M4A \0\0\0\0");
        let mdat = mp4_box(b"mdat", &[0u8; 64]);
        let moov = mp4_box(b"moov", &moov_children.concat());
        if moov_last {
            [ftyp, mdat, moov].concat()
        } else {
            [ftyp, moov, mdat].concat()
        }
    }

    #[test]
    fn the_edit_list_gives_delay_and_length() {
        let mvhd = mp4_box(b"mvhd", &header_v0(1_000));
        let trak = sound_trak(Some(elst_v0(&[(-1i32 as u32, -1), (500, 1_024)])));
        for moov_last in [false, true] {
            let g = read(&mut Cursor::new(file(
                &[mvhd.clone(), trak.clone()],
                moov_last,
            )))
            .unwrap()
            .unwrap();
            assert_eq!(g.frames(44_100), (1_024, Some(22_050)));
        }
    }

    #[test]
    fn itunsmpb_wins_over_the_edit_list() {
        let mvhd = mp4_box(b"mvhd", &header_v0(1_000));
        let trak = sound_trak(Some(elst_v0(&[(500, 1_024)])));
        let tag = [
            mp4_box(b"mean", &full(0, b"com.apple.iTunes")),
            mp4_box(b"name", &full(0, b"iTunSMPB")),
            mp4_box(
                b"data",
                b"\0\0\0\x01\0\0\0\0 00000000 00000840 000001CA 0000000000005622 00000000",
            ),
        ]
        .concat();
        let ilst = mp4_box(b"ilst", &mp4_box(b"----", &tag));
        let udta = mp4_box(b"udta", &mp4_box(b"meta", &full(0, &ilst)));
        let g = read(&mut Cursor::new(file(&[mvhd, trak, udta], true)))
            .unwrap()
            .unwrap();
        assert_eq!(
            g,
            Gapless::Smpb {
                delay: 0x840,
                valid: 0x5622
            }
        );
        assert_eq!(g.frames(48_000), (2_112, Some(22_050)));
    }

    #[test]
    fn other_files_and_tracks_without_edits_have_none() {
        let mvhd = mp4_box(b"mvhd", &header_v0(1_000));
        let no_edit = file(&[mvhd, sound_trak(None)], false);
        assert_eq!(read(&mut Cursor::new(no_edit)).unwrap(), None);
        let wav = crate::test_media::ramp_wav(44_100, 2, 16, 100);
        assert_eq!(read(&mut Cursor::new(wav)).unwrap(), None);
        assert_eq!(read(&mut Cursor::new(Vec::<u8>::new())).unwrap(), None);
        // A truncated box never panics.
        let mut broken = file(&[mp4_box(b"mvhd", &header_v0(1_000))], false);
        broken.truncate(30);
        assert_eq!(read(&mut Cursor::new(broken)).ok().flatten(), None);
    }
}
