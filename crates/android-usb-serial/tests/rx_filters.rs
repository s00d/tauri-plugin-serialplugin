//! RX filter properties: FTDI header strip and inline XON/XOFF return the payload
//! for any max packet size, whatever way the stream is split into reads.

use super::common::*;
use android_usb_serial::rx_filter::{
    strip_ftdi_header, ChainedRxFilter, FtdiHeaderFilter, RxFilter, XonXoffRxFilter,
};
use android_usb_serial::xonxoff::XonXoffFilter;

/// FTDI wire format: every `mps` packet starts with two status bytes; an empty
/// payload is a status-only packet.
fn ftdi_wire(payload: &[u8], mps: usize, status: [u8; 2]) -> Vec<u8> {
    let mut wire = vec![];
    for piece in payload.chunks(mps - 2) {
        wire.extend(status);
        wire.extend(piece);
    }
    if payload.is_empty() {
        wire.extend(status);
    }
    wire
}

/// Split `data` at the given cut points (taken modulo the length, sorted).
fn split<'a>(data: &'a [u8], cuts: &[usize], align: usize) -> Vec<&'a [u8]> {
    let mut points: Vec<usize> = cuts
        .iter()
        .map(|c| (c % (data.len() + 1)) / align * align)
        .collect();
    points.push(0);
    points.push(data.len());
    points.sort_unstable();
    points.dedup();
    points.windows(2).map(|w| &data[w[0]..w[1]]).collect()
}

fn byte() -> impl Strategy<Value = u8> {
    prop_oneof![4 => any::<u8>(), 1 => Just(CHAR_XON), 1 => Just(CHAR_XOFF)]
}

#[test]
fn ftdi_strip_returns_the_payload_for_any_packet_size() {
    run_cases(
        256,
        (
            prop::collection::vec(any::<u8>(), 0..2000),
            3usize..1100,
            any::<[u8; 2]>(),
            0usize..3,
            prop::collection::vec(any::<usize>(), 0..6),
        ),
        |(payload, mps, status, tail, cuts)| {
            let mut wire = ftdi_wire(&payload, mps, status);
            // A trailing status-only (or truncated status) packet carries no data.
            if wire.len() % mps == 0 {
                wire.extend(&status[..tail]);
            }
            prop_assert_eq!(&strip_ftdi_header(&wire, mps), &payload);
            prop_assert_eq!(&FtdiHeaderFilter::new(mps as u16).filter(&wire), &payload);
            // Reads end on packet boundaries; how many packets each read holds is irrelevant.
            let joined: Vec<u8> = split(&wire, &cuts, mps)
                .into_iter()
                .flat_map(|chunk| strip_ftdi_header(chunk, mps))
                .collect();
            prop_assert_eq!(&joined, &payload);
            Ok(())
        },
    );
}

#[test]
fn ftdi_strip_with_tiny_packets_keeps_nothing() {
    run_cases(
        512,
        (prop::collection::vec(any::<u8>(), 0..64), 0usize..3),
        |(wire, mps)| {
            prop_assert!(strip_ftdi_header(&wire, mps).is_empty());
            Ok(())
        },
    );
}

#[test]
fn xonxoff_drops_only_control_bytes_however_split() {
    run_cases(
        128,
        (
            prop::collection::vec(byte(), 0..200),
            prop::collection::vec(any::<usize>(), 0..6),
            any::<bool>(),
        ),
        |(input, cuts, enabled)| {
            let want: Vec<u8> = if enabled {
                input
                    .iter()
                    .copied()
                    .filter(|&b| b != CHAR_XON && b != CHAR_XOFF)
                    .collect()
            } else {
                input.clone()
            };
            let mut f = XonXoffFilter::new(enabled);
            let mut rx = XonXoffRxFilter::new(enabled);
            let mut got = vec![];
            let mut got_rx = vec![];
            for chunk in split(&input, &cuts, 1) {
                got.extend(f.filter(chunk));
                got_rx.extend(rx.filter(chunk));
            }
            prop_assert_eq!(&got, &want);
            prop_assert_eq!(&got_rx, &want);
            let last_ctrl = input
                .iter()
                .rev()
                .find(|&&b| b == CHAR_XON || b == CHAR_XOFF);
            prop_assert_eq!(f.paused, enabled && last_ctrl == Some(&CHAR_XOFF));
            Ok(())
        },
    );
}

#[test]
fn filter_chain_is_header_strip_then_xonxoff() {
    run_cases(
        64,
        (
            prop::collection::vec(byte(), 0..300),
            3usize..200,
            any::<[u8; 2]>(),
            prop::collection::vec(any::<usize>(), 0..4),
        ),
        |(payload, mps, status, cuts)| {
            let wire = ftdi_wire(&payload, mps, status);
            let want: Vec<u8> = payload
                .iter()
                .copied()
                .filter(|&b| b != CHAR_XON && b != CHAR_XOFF)
                .collect();
            let stages = || -> Vec<Box<dyn RxFilter>> {
                vec![
                    Box::new(FtdiHeaderFilter::new(mps as u16)),
                    Box::new(XonXoffRxFilter::new(true)),
                ]
            };
            let mut chain = ChainedRxFilter::new(stages());
            let mut list = stages();
            let mut got_chain = vec![];
            let mut got_list = vec![];
            for chunk in split(&wire, &cuts, mps) {
                got_chain.extend(chain.filter(chunk));
                got_list.extend(apply_filters(&mut list, chunk));
            }
            prop_assert_eq!(&got_chain, &want);
            prop_assert_eq!(&got_list, &want);
            Ok(())
        },
    );
}
