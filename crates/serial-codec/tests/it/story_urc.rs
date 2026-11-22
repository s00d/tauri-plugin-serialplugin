//! Stories: what an app learns from the lines of a modem.

use serial_codec::at::urc_lines_from_drain;
use serial_codec::urc::*;

/// Given a modem that is switched on
/// When its URCs are drained: it asks for nothing, searches, registers, reports its bearer
/// Then each line dispatches to its typed value in the JSON the frontend gets; results, RING,
/// `+CUSD`, read responses and half lines dispatch to nothing
#[test]
fn a_modem_attaches() {
    let raw = b"\r\nRDY\r\n\r\n+CPIN: READY\r\n\r\n+CEREG: 2\r\n\r\n+CEREG: 5,\"00C3\",\"0000A13B\",7\r\n\r\n+CGEV: ME PDN ACT 1\r\n\r\n+CGEV: NW ACT 1,2,1\r\n";
    let lines = urc_lines_from_drain(raw, &[]);
    assert_eq!(lines.len(), 5);
    assert_eq!(Cpin::parse(&lines[0]), Some(Cpin::Ready));
    let json: Vec<_> = lines[1..]
        .iter()
        .map(|l| serde_json::to_value(parse_any(l).unwrap()).unwrap())
        .collect();
    assert_eq!(json[0]["data"]["stat"], "searching");
    assert_eq!(json[1]["urc"], "registration");
    assert_eq!(json[1]["data"]["kind"], "cereg");
    assert_eq!(json[1]["data"]["stat"], "roaming");
    assert_eq!(json[1]["data"]["act"], "eutran");
    assert_eq!(json[1]["data"]["ci"], 0xA13B);
    assert_eq!(json[1]["data"]["rejectCause"], serde_json::Value::Null);
    assert_eq!(json[2]["data"]["event"]["event"], "mePdnAct");
    assert_eq!(json[3]["data"]["event"]["event"], "nwAct");
    assert_eq!(json[3]["data"]["event"]["pCid"], 1);
    assert_eq!(json[3]["data"]["raw"], "NW ACT 1,2,1");

    for l in [
        "",
        "OK",
        "RING",
        "+CUSD: 0,\"x\",15",
        "+CSQ: 1,2",
        "+CMTI",
        "é+",
    ] {
        assert_eq!(parse_any(l), None, "{l}");
    }
}

/// Given a phone that rings while `AT+CLIP?`, `AT+CCWA?` and `AT+CLCC` are asked
/// When the caller id, a second waiting call and the call list arrive
/// Then the URCs give the numbers, the read responses are no URCs, and the list shows both calls
#[test]
fn an_incoming_call() {
    let Some(Urc::Clip(c)) = parse_any("+CLIP: \"+10000000001\",145,,,\"Anna\",0") else {
        panic!()
    };
    assert_eq!(
        (c.number.as_str(), c.alpha.as_deref()),
        ("+10000000001", Some("Anna"))
    );
    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(
        (&json["type"], &json["cliValidity"]),
        (&145.into(), &0.into())
    );
    for read in ["+CLIP: 1,1", "+COLP: 0,2", "+CCWA: 1,1"] {
        assert_eq!(parse_any(read), None, "{read}");
    }
    let Some(Urc::Ccwa(w)) = parse_any("+CCWA: 10000000002,129,1") else {
        panic!()
    };
    assert_eq!((w.number.as_str(), w.class), ("10000000002", Some(1)));
    let list = [
        "+CLCC: 1,1,0,0,0,\"+10000000001\",145,\"Anna\"",
        "+CLCC: 2,1,5,0,0,\"10000000002\",129",
    ];
    let calls: Vec<_> = list.iter().map(|l| Clcc::parse(l).unwrap().stat).collect();
    assert_eq!(calls, [CallState::Active, CallState::Waiting]);
}

/// Given `AT+COPS=?` answered with an empty short name, a stray field behind a closed entry and
/// the mode and format lists at the end
/// When the list is parsed
/// Then the operators come out with the empty name as none, the stray field gives no access
/// technology (27.007 has none there), and the read response is another command's form
#[test]
fn an_operator_scan() {
    let l = "+COPS: (2,\"A\",\"A\",\"310260\"),2),(1,\"B\",\"\",\"310410\",0),,(0,1),(0,2)";
    let ops = CopsList::parse(l).unwrap().operators;
    let got: Vec<_> = ops
        .iter()
        .map(|o| (o.stat, o.short.as_deref(), o.act))
        .collect();
    assert_eq!(
        got,
        [
            (OperatorStatus::Current, Some("A"), None),
            (OperatorStatus::Available, None, Some(AccessTech::Gsm)),
        ]
    );
    assert_eq!(Cops::parse(l), None);
    assert_eq!(CopsList::parse("+COPS: 0,0,\"X\""), None);
    assert_eq!(
        Cops::parse("+COPS: 0,0,\"X\"").unwrap().oper.as_deref(),
        Some("X")
    );
}
