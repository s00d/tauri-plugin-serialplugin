//! Model: endpoint kinds and control requests (USB 2.0 §9.3, §9.6.6).

use super::common::*;
use android_usb_serial::transport::{
    is_device_recipient, is_interface_recipient, parse_control_recipient, ControlRequest,
};

/// Every address / attribute / request-type octet: the direction and transfer type decide the
/// endpoint kind, the low five bits the recipient. Any request carries its fields, its length
/// and the type octet of its constructor.
#[test]
fn endpoints_and_requests() {
    for (address, attributes) in (0..=255u8).flat_map(|a| (0..=255u8).map(move |t| (a, t))) {
        let e = ep(address, attributes);
        let input = address >= 0x80;
        assert_eq!(e.direction(), if input { 0x80 } else { 0 });
        assert_eq!(e.is_bulk_in(), input && attributes % 4 == 2);
        assert_eq!(e.is_bulk_out(), !input && attributes % 4 == 2);
        assert_eq!(e.is_interrupt_in(), input && attributes % 4 == 3);
    }
    for rt in 0..=255u8 {
        assert_eq!(parse_control_recipient(rt), (rt % 32, rt >= 0x80));
        assert_eq!(is_device_recipient(rt), rt % 32 == 0);
        assert_eq!(is_interface_recipient(rt), rt % 32 == 1);
    }
    let case = (any::<u8>(), any::<u16>(), any::<u16>(), 0usize..64);
    run_cases(256, case, |(request, value, index, len)| {
        let data = vec![7u8; len];
        let reqs = [
            (
                ControlRequest::vendor_out(request, value, index, data.clone()),
                0x40,
                7,
            ),
            (
                ControlRequest::vendor_in(request, value, index, len),
                0xC0,
                0,
            ),
            (
                ControlRequest::class_out(request, value, index, data),
                0x21,
                7,
            ),
            (
                ControlRequest::class_in(request, value, index, len),
                0xA1,
                0,
            ),
        ];
        for (req, rt, fill) in reqs {
            prop_assert_eq!(
                (req.request_type, req.request, req.value, req.index),
                (rt, request, value, index)
            );
            prop_assert_eq!(req.data, vec![fill; len]);
            prop_assert!(req.timeout_ms > 0);
        }
        Ok(())
    });
}
