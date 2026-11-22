//! Story: a multiplexer session on one link.

use serial_codec::cmux::{
    encode_address, encode_frame, encode_uih, Deframer, DISC, DM, FLAG, MAX_INFO, PF, SABM, UA, UIH,
};

/// Given a modem that has just entered multiplexer mode
/// When DLCI 0 and 1 open, data flows with AT text and a full 15-bit info field, the line then
/// carries 1 MiB of `F9 00` noise (every flag a false frame start), and the session closes
/// Then every frame arrives with its DLCI, kind and payload, the deframer is back in step on the
/// first frame after the noise, an info field one octet longer panics instead of encoding length
/// 0, and the one address the basic option cannot carry (DLCI 62 with C/R 0 is the flag octet)
/// is not delivered
#[test]
fn a_multiplexer_session() {
    let mut rx = Deframer::with_max_info(MAX_INFO);
    let mut next = |bytes: &[u8]| -> Vec<(u8, bool, u8, Vec<u8>)> {
        let frames = rx.feed(bytes);
        frames
            .into_iter()
            .map(|f| (f.dlci, f.cr, f.kind, f.payload))
            .collect()
    };
    for dlci in [0, 1] {
        let sabm = encode_frame(dlci, true, SABM | PF, &[]);
        assert_eq!(next(&sabm), [(dlci, true, SABM, vec![])]);
        let ua = encode_frame(dlci, true, UA | PF, &[]);
        assert_eq!(next(&ua), [(dlci, true, UA, vec![])]);
    }
    assert_eq!(
        next(&encode_uih(1, b"AT\r")),
        [(1, true, UIH, b"AT\r".to_vec())]
    );
    let info = vec![0x55; MAX_INFO];
    assert_eq!(
        next(&encode_frame(1, true, UIH, &info)),
        [(1, true, UIH, info)]
    );
    let too_long = std::panic::catch_unwind(|| encode_frame(1, true, UIH, &[0; MAX_INFO + 1]));
    assert!(too_long.is_err());

    let noise = [FLAG, 0x00].repeat(1 << 19);
    assert!(next(&noise).is_empty());
    assert_eq!(
        next(&encode_uih(1, b"good")),
        [(1, true, UIH, b"good".to_vec())]
    );

    assert_eq!(encode_address(62, false), FLAG);
    let mut other = Deframer::default();
    assert!(other.feed(&encode_frame(62, false, UIH, b"x")).is_empty());
    let mut other = Deframer::default();
    assert_eq!(other.feed(&encode_frame(62, true, UIH, b"x")).len(), 1);

    let disc = encode_frame(1, true, DISC | PF, &[]);
    assert_eq!(next(&disc), [(1, true, DISC, vec![])]);
    let dm = encode_frame(1, false, DM | PF, &[]);
    assert_eq!(dm[..4], [FLAG, 0x05, 0x1F, 0x01]);
    assert_eq!(next(&dm), [(1, false, DM, vec![])]);
}
