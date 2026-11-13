//! Public pure helpers: endpoint kinds, control-request constructors, recipient
//! parsing, the adapter's write timeout and `serialport` conversions — as
//! properties over all inputs.

use super::common::*;
use android_usb_serial::transport::{
    is_device_recipient, is_interface_recipient, parse_control_recipient, ControlRequest,
};

#[test]
fn endpoint_kind_needs_direction_and_transfer_type() {
    run_cases(256, (any::<u8>(), any::<u8>()), |(address, attributes)| {
        let e = EndpointInfo {
            address,
            attributes,
            max_packet_size: 64,
            interval: 0,
        };
        let input = address & 0x80 != 0;
        prop_assert_eq!(e.direction(), address & 0x80);
        prop_assert_eq!(e.is_bulk_in(), input && attributes & 3 == 2);
        prop_assert_eq!(e.is_bulk_out(), !input && attributes & 3 == 2);
        prop_assert_eq!(e.is_interrupt_in(), input && attributes & 3 == 3);
        Ok(())
    });
}

#[test]
fn control_requests_carry_type_recipient_and_length() {
    let case = (any::<u8>(), any::<u16>(), any::<u16>(), 0usize..64);
    run_cases(256, case, |(request, value, index, len)| {
        let data = vec![7u8; len];
        let reqs = [
            (
                ControlRequest::vendor_out(request, value, index, data.clone()),
                0x40,
                len,
            ),
            (
                ControlRequest::vendor_in(request, value, index, len),
                0xC0,
                len,
            ),
            (
                ControlRequest::class_out(request, value, index, data.clone()),
                0x21,
                len,
            ),
            (
                ControlRequest::class_in(request, value, index, len),
                0xA1,
                len,
            ),
        ];
        for (req, rt, wlen) in reqs {
            prop_assert_eq!(
                (req.request_type, req.request, req.value, req.index),
                (rt, request, value, index)
            );
            prop_assert_eq!(req.data.len(), wlen);
            prop_assert!(req.timeout_ms > 0);
            let (recipient, dir_in) = parse_control_recipient(req.request_type);
            prop_assert_eq!(dir_in, rt & 0x80 != 0);
            prop_assert_eq!(recipient, rt & 0x1f);
            prop_assert_eq!(is_device_recipient(rt), rt & 0x1f == 0);
            prop_assert_eq!(is_interface_recipient(rt), rt & 0x1f == 1);
        }
        Ok(())
    });
}

#[test]
fn recipient_parsing_reads_the_low_bits_and_direction() {
    run_cases(256, any::<u8>(), |rt| {
        prop_assert_eq!(parse_control_recipient(rt), (rt & 0x1f, rt & 0x80 != 0));
        prop_assert_eq!(is_device_recipient(rt), rt & 0x1f == 0);
        prop_assert_eq!(is_interface_recipient(rt), rt & 0x1f == 1);
        Ok(())
    });
}

/// Given every `UsbSerialError` variant
/// When it is displayed
/// Then the text names the kind (and carries the detail for the ones that have it).
#[test]
fn error_display_names_the_kind() {
    let cases = [
        (UsbSerialError::Unsupported("x".into()), "unsupported: x"),
        (UsbSerialError::Io("x".into()), "io: x"),
        (UsbSerialError::TimedOut, "timed out"),
        (UsbSerialError::Disconnected, "disconnected"),
        (UsbSerialError::Cancelled, "cancelled"),
        (UsbSerialError::ProbeFailed("x".into()), "probe failed: x"),
    ];
    for (e, text) in cases {
        assert_eq!(e.to_string(), text);
    }
}

#[cfg(feature = "serialport-compat")]
mod compat {
    use super::*;
    use android_usb_serial::serialport_compat::{
        chunk_write_timeout_ms, flow_from_serialport, line_config_from_serialport,
    };
    use serialport::{DataBits as D, FlowControl as F, Parity as P, StopBits as S};

    #[test]
    fn chunk_write_timeout_is_clamped_and_floored_at_two_seconds() {
        run_cases(256, any::<u64>(), |ms| {
            let got = chunk_write_timeout_ms(Duration::from_millis(ms));
            prop_assert_eq!(u64::from(got), ms.clamp(2000, 600_000));
            Ok(())
        });
    }

    #[test]
    fn serialport_settings_convert_field_by_field() {
        let case = (
            any::<u32>(),
            prop::sample::select(vec![D::Five, D::Six, D::Seven, D::Eight]),
            prop::sample::select(vec![P::None, P::Odd, P::Even]),
            prop::sample::select(vec![S::One, S::Two]),
            prop::sample::select(vec![F::None, F::Hardware, F::Software]),
        );
        run_cases(256, case, |(baud, d, p, s, f)| {
            let cfg = line_config_from_serialport(baud, d, p, s);
            prop_assert_eq!(cfg.baud_rate, baud);
            let data = [
                (D::Five, DataBits::Five),
                (D::Six, DataBits::Six),
                (D::Seven, DataBits::Seven),
                (D::Eight, DataBits::Eight),
            ];
            prop_assert_eq!(
                Some(cfg.data_bits),
                data.iter().find(|x| x.0 == d).map(|x| x.1)
            );
            let parity = [
                (P::None, Parity::None),
                (P::Odd, Parity::Odd),
                (P::Even, Parity::Even),
            ];
            prop_assert_eq!(
                Some(cfg.parity),
                parity.iter().find(|x| x.0 == p).map(|x| x.1)
            );
            prop_assert_eq!(
                cfg.stop_bits,
                if s == S::Two {
                    StopBits::Two
                } else {
                    StopBits::One
                }
            );
            let flow = [
                (F::None, FlowControl::None),
                (F::Hardware, FlowControl::RtsCts),
                (F::Software, FlowControl::XonXoff),
            ];
            prop_assert_eq!(
                Some(flow_from_serialport(f)),
                flow.iter().find(|x| x.0 == f).map(|x| x.1)
            );
            Ok(())
        });
    }
}
