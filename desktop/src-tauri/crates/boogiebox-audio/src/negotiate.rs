//! Exclusive-mode format choice (plan D8, D9): the track's own rate in a container that keeps every bit;
//! failing that, the nearest rate the device takes (same rate family first) with resampling. Never silent:
//! the chosen format and what it costs show up in the track's path.

use crate::convert::{DeviceFormat, SampleKind};
use crate::decode::StreamInfo;

/// Rates probed per device.
pub const RATES: [u32; 8] = [
    44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000,
];

/// Sample kinds probed per device, with the bits they store and keep.
pub const KINDS: [SampleKind; 4] = [
    SampleKind::I16,
    SampleKind::I24Packed,
    SampleKind::I24In32,
    SampleKind::I32,
];

/// What a device accepts in exclusive mode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceCaps {
    /// Every accepted (rate, channels, kind).
    pub formats: Vec<DeviceFormat>,
}

impl DeviceCaps {
    fn takes(&self, rate: u32, channels: u16, kind: SampleKind) -> bool {
        self.formats
            .iter()
            .any(|f| f.sample_rate == rate && f.channels == channels && f.kind == kind)
    }

    fn rates(&self, channels: u16) -> Vec<u32> {
        let mut r: Vec<u32> = self
            .formats
            .iter()
            .filter(|f| f.channels == channels)
            .map(|f| f.sample_rate)
            .collect();
        r.sort_unstable();
        r.dedup();
        r
    }
}

/// Containers in order of preference for a source of `bits` (None: lossy, decoded at full precision).
/// Every container at least as wide as the source keeps it bit for bit (samples are left-justified).
fn kinds_for(bits: Option<u16>) -> [SampleKind; 4] {
    match bits {
        Some(b) if b <= 16 => [
            SampleKind::I16,
            SampleKind::I24In32,
            SampleKind::I24Packed,
            SampleKind::I32,
        ],
        Some(b) if b <= 24 => [
            SampleKind::I24In32,
            SampleKind::I24Packed,
            SampleKind::I32,
            SampleKind::I16,
        ],
        _ => [
            SampleKind::I32,
            SampleKind::I24In32,
            SampleKind::I24Packed,
            SampleKind::I16,
        ],
    }
}

fn family(rate: u32) -> u32 {
    if rate.is_multiple_of(11_025) {
        44_100
    } else {
        48_000
    }
}

/// Picks the exclusive format for a stream, or `None` when the device takes nothing usable.
pub fn choose(caps: &DeviceCaps, info: &StreamInfo) -> Option<DeviceFormat> {
    let channels = if caps.rates(info.channels).is_empty() {
        2
    } else {
        info.channels
    };
    let kinds = kinds_for(info.bits);
    let pick_kind = |rate: u32| kinds.into_iter().find(|k| caps.takes(rate, channels, *k));
    let rates = caps.rates(channels);
    // D8: the track's own rate. D9: same family upwards, then same family downwards, then the closest.
    let src = info.sample_rate;
    let fam = family(src);
    let mut order: Vec<u32> = vec![src];
    order.extend(
        rates
            .iter()
            .copied()
            .filter(|r| *r > src && family(*r) == fam),
    );
    order.extend(
        rates
            .iter()
            .rev()
            .copied()
            .filter(|r| *r < src && family(*r) == fam),
    );
    let mut rest: Vec<u32> = rates
        .iter()
        .copied()
        .filter(|r| family(*r) != fam)
        .collect();
    rest.sort_by_key(|r| r.abs_diff(src));
    order.extend(rest);
    order.into_iter().find_map(|rate| {
        pick_kind(rate).map(|kind| DeviceFormat {
            sample_rate: rate,
            channels,
            kind,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(rates: &[u32], kinds: &[SampleKind]) -> DeviceCaps {
        DeviceCaps {
            formats: rates
                .iter()
                .flat_map(|r| {
                    kinds.iter().map(move |k| DeviceFormat {
                        sample_rate: *r,
                        channels: 2,
                        kind: *k,
                    })
                })
                .collect(),
        }
    }

    fn info(rate: u32, bits: Option<u16>, channels: u16) -> StreamInfo {
        StreamInfo {
            sample_rate: rate,
            channels,
            bits,
            codec: "FLAC".into(),
            frames: None,
        }
    }

    const FIIO: [SampleKind; 3] = [SampleKind::I16, SampleKind::I24In32, SampleKind::I32];

    #[test]
    fn own_rate_in_the_tightest_container_that_keeps_every_bit() {
        let fiio = caps(&RATES, &FIIO);
        let f = choose(&fiio, &info(44_100, Some(16), 2)).unwrap();
        assert_eq!((f.sample_rate, f.kind), (44_100, SampleKind::I16));
        let f = choose(&fiio, &info(192_000, Some(24), 2)).unwrap();
        assert_eq!((f.sample_rate, f.kind), (192_000, SampleKind::I24In32));
        let f = choose(&fiio, &info(44_100, None, 2)).unwrap();
        assert_eq!(f.kind, SampleKind::I32, "lossy: widest");
        // A device without 16-bit keeps 16-bit sources exact in a wider container.
        let wide = caps(&[44_100], &[SampleKind::I24In32]);
        assert_eq!(
            choose(&wide, &info(44_100, Some(16), 2)).unwrap().kind,
            SampleKind::I24In32
        );
        // A 16-bit-only device: the only option, narrower than the source.
        let narrow = caps(&[96_000], &[SampleKind::I16]);
        assert_eq!(
            choose(&narrow, &info(96_000, Some(24), 2)).unwrap().kind,
            SampleKind::I16
        );
    }

    #[test]
    fn missing_rates_fall_back_within_the_family_first() {
        // Realtek S/PDIF from the phase 0 spike: no 176.4 kHz.
        let spdif = caps(
            &[44_100, 48_000, 88_200, 96_000, 192_000],
            &[SampleKind::I16, SampleKind::I24In32],
        );
        let f = choose(&spdif, &info(176_400, Some(24), 2)).unwrap();
        assert_eq!(f.sample_rate, 88_200, "44.1 family, nearest below");
        let f = choose(&spdif, &info(384_000, Some(24), 2)).unwrap();
        assert_eq!(f.sample_rate, 192_000);
        let f = choose(&spdif, &info(22_050, Some(16), 2)).unwrap();
        assert_eq!(f.sample_rate, 44_100, "same family upwards first");
        // HDMI monitor from the spike: 48 kHz 16-bit only.
        let hdmi = caps(&[48_000], &[SampleKind::I16]);
        let f = choose(&hdmi, &info(44_100, Some(16), 2)).unwrap();
        assert_eq!(
            (f.sample_rate, f.kind),
            (48_000, SampleKind::I16),
            "other family as a last resort"
        );
        assert!(choose(&DeviceCaps::default(), &info(44_100, Some(16), 2)).is_none());
    }

    #[test]
    fn channels_follow_the_device() {
        let stereo = caps(&[48_000], &FIIO);
        assert_eq!(
            choose(&stereo, &info(48_000, Some(16), 1))
                .unwrap()
                .channels,
            2
        );
        let mut six = caps(&[48_000], &FIIO);
        six.formats.push(DeviceFormat {
            sample_rate: 48_000,
            channels: 6,
            kind: SampleKind::I24In32,
        });
        assert_eq!(
            choose(&six, &info(48_000, Some(24), 6)).unwrap().channels,
            6
        );
    }
}
