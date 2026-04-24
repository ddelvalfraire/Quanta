use proptest::prelude::*;

use quanta_core_rs::bridge::*;
use quanta_core_rs::*;

fn arb_sender_wire() -> impl Strategy<Value = SenderWire> {
    prop_oneof![
        (".*", ".*", ".*").prop_map(|(ns, typ, id)| SenderWire::Actor {
            namespace: ns,
            typ,
            id,
        }),
        ".*".prop_map(SenderWire::Client),
        Just(SenderWire::System),
        Just(SenderWire::None),
    ]
}

fn arb_envelope_header() -> impl Strategy<Value = EnvelopeHeader> {
    (
        ".*",
        any::<u64>(),
        any::<u16>(),
        proptest::option::of(".*"),
        proptest::option::of(".*"),
        arb_sender_wire(),
        proptest::collection::vec((".*", ".*"), 0..4),
    )
        .prop_map(
            |(message_id, wall_us, logical, correlation_id, causation_id, sender, metadata)| {
                EnvelopeHeader {
                    message_id,
                    wall_us,
                    logical,
                    correlation_id,
                    causation_id,
                    sender,
                    metadata,
                }
            },
        )
}

fn arb_bridge_msg_type() -> impl Strategy<Value = BridgeMsgType> {
    prop_oneof![
        Just(BridgeMsgType::ActivateIsland),
        Just(BridgeMsgType::DeactivateIsland),
        Just(BridgeMsgType::PlayerJoin),
        Just(BridgeMsgType::PlayerLeave),
        Just(BridgeMsgType::EntityCommand),
        Just(BridgeMsgType::StateSync),
        Just(BridgeMsgType::Heartbeat),
        Just(BridgeMsgType::CapacityReport),
    ]
}

fn arb_bridge_header() -> impl Strategy<Value = BridgeHeader> {
    (
        arb_bridge_msg_type(),
        any::<u64>(),
        any::<u64>(),
        proptest::option::of(any::<[u8; 16]>()),
    )
        .prop_map(
            |(msg_type, sequence, timestamp, correlation_id)| BridgeHeader {
                msg_type,
                sequence,
                timestamp,
                correlation_id,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn envelope_header_roundtrip(
        header in arb_envelope_header(),
        payload in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let frame = encode_wire_frame(&header, &payload);
        let (decoded, decoded_payload) = decode_wire_frame(&frame).unwrap();
        prop_assert_eq!(&decoded, &header);
        prop_assert_eq!(decoded_payload, payload.as_slice());
    }

    #[test]
    fn bridge_header_roundtrip(
        header in arb_bridge_header(),
        payload in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let frame = encode_bridge_frame(&header, &payload);
        let (decoded, decoded_payload) = decode_bridge_frame(&frame).unwrap();
        prop_assert_eq!(&decoded, &header);
        prop_assert_eq!(decoded_payload, payload.as_slice());
    }
}
