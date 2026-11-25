//! Literal history contracts, structural rejection and event integrity tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_protocol::{
    HistoryDomain, HistoryDone, HistoryFilter, HistoryPage, HistoryRead, HistoryRecord,
    HistoryRequest, HistoryResponse, HistorySnapshot, ProtocolError,
};

const RECORD: [u8; 42] = [
    1, 0, 0, 0, 2, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 32, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 4, 0, 60, 0, 0, 1, 0xbd, 0x1a, 0x16, 0x3a,
];
const DOMAIN: [u8; 21] = [
    32, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0,
];
const OPEN_RESPONSE: [u8; 34] = [
    0, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
    60, 0,
];
const DATASET: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
fn unfiltered() -> HistoryFilter {
    HistoryFilter {
        domain: 32,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    }
}
fn snapshot() -> HistorySnapshot {
    HistorySnapshot {
        snapshot: 1,
        epoch: 3,
        first_id: 4,
        last_id: 4,
        count: 1,
        lease_s: 60,
    }
}

#[test]
fn all_eight_request_opcodes_decode_and_encode_literal_contracts() {
    let cases: &[(HistoryRequest, &[u8])] = &[
        (HistoryRequest::Enumerate { start: 0, max: 8 }, &[0, 8]),
        (
            HistoryRequest::Open(unfiltered()),
            &[32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            HistoryRequest::Read {
                snapshot: 1,
                after_id: 0x0807_0605_0403_0201,
                max_records: 16,
            },
            &[1, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 16, 0],
        ),
        (
            HistoryRequest::Ack {
                snapshot: 1,
                epoch: 3,
                upto_id: 0x0807_0605_0403_0201,
            },
            &[1, 0, 0, 0, 3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8],
        ),
        (HistoryRequest::Close { snapshot: 1 }, &[1, 0, 0, 0]),
        (
            HistoryRequest::OpenV2(HistoryFilter {
                domain: 32,
                filter_flags: 7,
                start_utc_ms: -100,
                end_utc_ms: 100,
                record_type: 32,
                parent_session: 9,
            }),
            &[
                32, 7, 156, 255, 255, 255, 255, 255, 255, 255, 100, 0, 0, 0, 0, 0, 0, 0, 32, 0, 9,
                0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            HistoryRequest::Count(unfiltered()),
            &[
                32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            HistoryRequest::CancelRead {
                snapshot: 1,
                stream_id: 2,
            },
            &[1, 0, 0, 0, 2, 0],
        ),
    ];
    for (value, literal) in cases {
        let opcode = value.opcode();
        assert_eq!(HistoryRequest::decode(opcode, literal), Ok(*value));
        let mut out = [0xa5; 64];
        let n = value.encode(&mut out).unwrap();
        assert_eq!(&out[..n], *literal);
        assert_eq!(out[n], 0xa5);
        for cut in 0..literal.len() {
            assert!(HistoryRequest::decode(opcode, &literal[..cut]).is_err());
        }
        let mut extra = literal.to_vec();
        extra.push(0);
        assert!(HistoryRequest::decode(opcode, &extra).is_err());
        assert_eq!(
            value.encode(&mut out[..literal.len() - 1]),
            Err(ProtocolError::Capacity)
        );
    }
}

#[test]
fn request_filters_reject_reserved_bits_disabled_values_and_reversed_windows() {
    let bad = [
        HistoryRequest::Open(HistoryFilter {
            filter_flags: 2,
            ..unfiltered()
        }),
        HistoryRequest::Open(HistoryFilter {
            record_type: 1,
            ..unfiltered()
        }),
        HistoryRequest::Open(HistoryFilter {
            start_utc_ms: 1,
            ..unfiltered()
        }),
        HistoryRequest::OpenV2(HistoryFilter {
            filter_flags: 8,
            ..unfiltered()
        }),
        HistoryRequest::Count(HistoryFilter {
            record_type: 1,
            ..unfiltered()
        }),
        HistoryRequest::Count(HistoryFilter {
            parent_session: 1,
            ..unfiltered()
        }),
        HistoryRequest::OpenV2(HistoryFilter {
            filter_flags: 1,
            start_utc_ms: 100,
            end_utc_ms: 100,
            ..unfiltered()
        }),
        HistoryRequest::OpenV2(HistoryFilter {
            filter_flags: 1,
            start_utc_ms: 100,
            end_utc_ms: -100,
            ..unfiltered()
        }),
        HistoryRequest::Open(HistoryFilter {
            domain: 255,
            ..unfiltered()
        }),
        HistoryRequest::Enumerate { start: 255, max: 1 },
        HistoryRequest::Enumerate { start: 0, max: 0 },
        HistoryRequest::Enumerate { start: 0, max: 9 },
        HistoryRequest::Read {
            snapshot: 0,
            after_id: 0,
            max_records: 1,
        },
        HistoryRequest::Read {
            snapshot: 1,
            after_id: 0,
            max_records: 0,
        },
        HistoryRequest::Read {
            snapshot: 1,
            after_id: 0,
            max_records: 17,
        },
        HistoryRequest::Ack {
            snapshot: 1,
            epoch: 0,
            upto_id: 1,
        },
        HistoryRequest::CancelRead {
            snapshot: 1,
            stream_id: 0,
        },
    ];
    for request in bad {
        assert!(request.encode(&mut [0; 64]).is_err(), "{request:?}");
    }
    assert!(HistoryRequest::decode(9, &[]).is_err());
    assert!(
        HistoryRequest::decode(
            7,
            &[
                32, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        )
        .is_err()
    );
}

#[test]
fn all_eight_success_responses_include_exact_common_status_only_once() {
    let page = HistoryPage {
        dataset_id: DATASET,
        next: 255,
        entries: &DOMAIN,
    };
    let page_literal = [
        0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 255, 1, 32, 3, 0, 0, 0,
        4, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0,
    ];
    let cases: &[(HistoryResponse<'_>, &[u8])] = &[
        (HistoryResponse::Enumerate(page), &page_literal),
        (HistoryResponse::Open(snapshot()), &OPEN_RESPONSE),
        (
            HistoryResponse::Read(HistoryRead {
                stream_id: 2,
                end_after_id: 4,
                eof: true,
                admitted: 1,
            }),
            &[0, 0, 0, 0, 2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0],
        ),
        (
            HistoryResponse::Ack { committed_id: 4 },
            &[0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            HistoryResponse::Close {
                last_data_message_id: 2,
            },
            &[0, 0, 0, 0, 2, 0],
        ),
        (HistoryResponse::OpenV2(snapshot()), &OPEN_RESPONSE),
        (
            HistoryResponse::Count { epoch: 3, count: 1 },
            &[0, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0],
        ),
        (
            HistoryResponse::CancelRead {
                last_data_message_id: 0,
            },
            &[0, 0, 0, 0, 0, 0],
        ),
    ];
    for (value, literal) in cases {
        assert_eq!(HistoryResponse::decode(value.opcode(), literal), Ok(*value));
        let mut out = [0xa5; 64];
        let n = value.encode(&mut out).unwrap();
        assert_eq!(&out[..n], *literal);
        assert_eq!(out[n], 0xa5);
        for cut in 0..literal.len() {
            assert!(HistoryResponse::decode(value.opcode(), &literal[..cut]).is_err());
        }
        let mut extra = literal.to_vec();
        extra.push(0);
        assert!(HistoryResponse::decode(value.opcode(), &extra).is_err());
        assert_eq!(
            value.encode(&mut out[..literal.len() - 1]),
            Err(ProtocolError::Capacity)
        );
    }
}

#[test]
fn response_failures_have_no_success_tail_and_pending_is_forbidden() {
    for opcode in 1..=8 {
        let reply = HistoryResponse::Failure {
            opcode,
            status: 5,
            reason: 0xff03,
        };
        assert_eq!(HistoryResponse::decode(opcode, &[5, 0, 3, 255]), Ok(reply));
        let mut out = [0xa5; 8];
        assert_eq!(reply.encode(&mut out), Ok(4));
        assert_eq!(&out[..4], &[5, 0, 3, 255]);
        assert_eq!(out[4], 0xa5);
        assert!(HistoryResponse::decode(opcode, &[5, 0, 3, 255, 0]).is_err());
        assert!(HistoryResponse::decode(opcode, &[16, 0, 0, 0]).is_err());
        assert!(HistoryResponse::decode(opcode, &[19, 0, 0, 0]).is_err());
    }
    let mut bad = OPEN_RESPONSE;
    bad[2] = 1;
    assert!(HistoryResponse::decode(2, &bad).is_err());
    assert!(
        HistoryResponse::Failure {
            opcode: 2,
            status: 0,
            reason: 0
        }
        .encode(&mut [0; 64])
        .is_err()
    );
}

#[test]
fn typed_domain_pages_reject_bad_order_count_lengths_and_sentinels() {
    let domain = HistoryDomain {
        domain: 32,
        epoch: 3,
        oldest_id: 4,
        newest_id: 4,
    };
    assert_eq!(HistoryDomain::decode(&DOMAIN), Ok(domain));
    let mut out = [0; 21];
    assert_eq!(domain.encode(&mut out), Ok(21));
    assert_eq!(out, DOMAIN);
    let page = HistoryPage {
        dataset_id: DATASET,
        next: 255,
        entries: &DOMAIN,
    };
    assert_eq!(page.domain(0), Some(domain));
    assert_eq!(page.domain(1), None);
    assert!(HistoryDomain::decode(&DOMAIN[..20]).is_err());
    let mut duplicate = [0; 42];
    duplicate[..21].copy_from_slice(&DOMAIN);
    duplicate[21..].copy_from_slice(&DOMAIN);
    for (next, entries) in [
        (32, &DOMAIN[..]),
        (255, &duplicate[..]),
        (255, &DOMAIN[..20]),
        (1, &[][..]),
    ] {
        assert!(
            HistoryResponse::Enumerate(HistoryPage {
                dataset_id: DATASET,
                next,
                entries
            })
            .encode(&mut [0; 256])
            .is_err()
        );
    }
    let mut too_many = [0; 189];
    for (i, entry) in too_many.chunks_exact_mut(21).enumerate() {
        entry.copy_from_slice(&DOMAIN);
        entry[0] = u8::try_from(i).unwrap();
    }
    assert!(
        HistoryResponse::Enumerate(HistoryPage {
            dataset_id: DATASET,
            next: 255,
            entries: &too_many
        })
        .encode(&mut [0; 256])
        .is_err()
    );
    for field in [0, 1, 5] {
        let mut bad = DOMAIN;
        if field == 0 {
            bad[0] = 255;
        } else {
            bad[field] = 0;
        }
        assert!(HistoryDomain::decode(&bad).is_err());
    }
    let empty = [
        0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 255, 0,
    ];
    assert_eq!(
        HistoryResponse::decode(1, &empty),
        Ok(HistoryResponse::Enumerate(HistoryPage {
            dataset_id: DATASET,
            next: 255,
            entries: &[]
        }))
    );
}

#[test]
fn response_snapshot_and_read_invariants_reject_inconsistent_empty_state() {
    let mut out = [0; 64];
    for snap in [
        HistorySnapshot {
            snapshot: 0,
            ..snapshot()
        },
        HistorySnapshot {
            epoch: 0,
            ..snapshot()
        },
        HistorySnapshot {
            first_id: 0,
            ..snapshot()
        },
        HistorySnapshot {
            last_id: 3,
            ..snapshot()
        },
        HistorySnapshot {
            count: 0,
            ..snapshot()
        },
    ] {
        assert!(HistoryResponse::Open(snap).encode(&mut out).is_err());
    }
    let empty = HistorySnapshot {
        first_id: 0,
        last_id: 0,
        count: 0,
        ..snapshot()
    };
    assert_eq!(HistoryResponse::Open(empty).encode(&mut out), Ok(34));
    assert!(
        HistoryResponse::Read(HistoryRead {
            stream_id: 0,
            end_after_id: 0,
            eof: true,
            admitted: 0
        })
        .encode(&mut out)
        .is_err()
    );
    assert!(
        HistoryResponse::Read(HistoryRead {
            stream_id: 1,
            end_after_id: 1,
            eof: true,
            admitted: 17
        })
        .encode(&mut out)
        .is_err()
    );
    let mut bad = [0, 0, 0, 0, 2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0];
    bad[14] = 2;
    assert!(HistoryResponse::decode(3, &bad).is_err());
}

#[test]
fn record_matches_literal_crc_3a161abd_and_borrows_payload() {
    let record = HistoryRecord::decode(&RECORD, 128).unwrap();
    assert_eq!(
        (
            record.snapshot,
            record.stream_id,
            record.epoch,
            record.record_id
        ),
        (1, 2, 3, 4)
    );
    assert_eq!(
        (
            record.record_type,
            record.schema,
            record.utc_ms,
            record.flags
        ),
        (32, 1, 0, 0)
    );
    assert_eq!(record.payload, &[60, 0, 0, 1]);
    assert_eq!(record.payload.as_ptr(), RECORD[34..38].as_ptr());
    let mut out = [0xa5; 128];
    assert_eq!(record.encode(&mut out, 128), Ok(42));
    assert_eq!(&out[..42], &RECORD);
    assert_eq!(out[42], 0xa5);
    assert_eq!(&out[38..42], &[0xbd, 0x1a, 0x16, 0x3a]);
}

#[test]
fn record_crc_and_declared_length_reject_every_single_bit_corruption() {
    for bit in 0..RECORD.len() * 8 {
        let mut bad = RECORD;
        bad[bit / 8] ^= 1 << (bit % 8);
        assert!(HistoryRecord::decode(&bad, 128).is_err(), "bit {bit}");
    }
    let mut bad = RECORD;
    bad[34] ^= 1;
    assert_eq!(
        HistoryRecord::decode(&bad, 128),
        Err(ProtocolError::Integrity)
    );
    for cut in 0..RECORD.len() {
        assert!(HistoryRecord::decode(&RECORD[..cut], 128).is_err());
    }
    let mut extra = RECORD.to_vec();
    extra.push(0);
    assert!(HistoryRecord::decode(&extra, 128).is_err());
    for cap in [0, 20, 127, 513] {
        assert!(HistoryRecord::decode(&RECORD, cap).is_err());
    }
}

#[test]
fn record_capacity_and_time_flags_bound_full_event_not_payload_only() {
    let record = HistoryRecord::decode(&RECORD, 128).unwrap();
    let full = HistoryRecord {
        payload: &[7; 90],
        ..record
    };
    let mut out = [0; 512];
    assert_eq!(full.encode(&mut out, 128), Ok(128));
    let over = HistoryRecord {
        payload: &[7; 91],
        ..record
    };
    assert!(over.encode(&mut out, 128).is_err());
    assert_eq!(
        record.encode(&mut out[..41], 128),
        Err(ProtocolError::Capacity)
    );
    for bad in [
        HistoryRecord { flags: 2, ..record },
        HistoryRecord {
            utc_ms: 1,
            ..record
        },
        HistoryRecord {
            snapshot: 0,
            ..record
        },
        HistoryRecord {
            stream_id: 0,
            ..record
        },
        HistoryRecord { epoch: 0, ..record },
        HistoryRecord {
            record_id: 0,
            ..record
        },
    ] {
        assert!(bad.encode(&mut out, 128).is_err());
    }
    let signed = HistoryRecord {
        flags: 1,
        utc_ms: i64::MIN,
        ..record
    };
    let n = signed.encode(&mut out, 128).unwrap();
    assert_eq!(
        HistoryRecord::decode(&out[..n], 128).unwrap().utc_ms,
        i64::MIN
    );
}

#[test]
fn done_is_exact_sixteen_byte_control_event_without_crc_or_common_prefix() {
    let literal = [2, 0, 1, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let done = HistoryDone {
        stream_id: 2,
        sent_count: 1,
        last_id: 4,
        status: 0,
        reason: 0,
    };
    assert_eq!(HistoryDone::decode(&literal), Ok(done));
    let mut out = [0xa5; 24];
    assert_eq!(done.encode(&mut out), Ok(16));
    assert_eq!(&out[..16], &literal);
    assert_eq!(out[16], 0xa5);
    for cut in 0..16 {
        assert!(HistoryDone::decode(&literal[..cut]).is_err());
    }
    let mut extra = literal.to_vec();
    extra.extend_from_slice(&[0; 4]);
    assert!(HistoryDone::decode(&extra).is_err());
    for bad in [
        HistoryDone {
            stream_id: 0,
            ..done
        },
        HistoryDone { status: 16, ..done },
        HistoryDone { status: 19, ..done },
        HistoryDone { reason: 1, ..done },
        HistoryDone {
            sent_count: 17,
            ..done
        },
    ] {
        assert!(bad.encode(&mut out).is_err());
    }
    assert_eq!(done.encode(&mut out[..15]), Err(ProtocolError::Capacity));
}

#[test]
fn borrowed_payload_debug_is_redacted_for_records_and_domain_pages() {
    let record = HistoryRecord::decode(&RECORD, 128).unwrap();
    let secret = b"private-history-payload";
    let debug = format!(
        "{:?}",
        HistoryRecord {
            payload: secret,
            ..record
        }
    );
    assert!(!debug.contains("private-history"));
    assert!(!debug.contains("112, 114, 105, 118"));
    assert!(debug.contains("payload_length"));
    let page = HistoryPage {
        dataset_id: DATASET,
        next: 255,
        entries: secret,
    };
    let debug = format!("{page:?}");
    assert!(!debug.contains("112, 114, 105, 118"));
}

#[test]
fn global_reasons_must_match_their_frozen_status_class() {
    for (status, reason) in [
        (5, 0xff01),
        (1, 0xff03),
        (5, 0xff05),
        (4, 0xff07),
        (3, 0xff08),
    ] {
        let reply = HistoryResponse::Failure {
            opcode: 4,
            status,
            reason,
        };
        assert!(
            reply.encode(&mut [0; 64]).is_err(),
            "status={status} reason={reason:x}"
        );
        let done = HistoryDone {
            stream_id: 1,
            sent_count: 0,
            last_id: 0,
            status,
            reason,
        };
        assert!(done.encode(&mut [0; 16]).is_err());
    }
    for (status, reason) in [
        (0x12, 0xff01),
        (0x12, 0xff02),
        (5, 0xff03),
        (5, 0xff04),
        (4, 0xff05),
        (4, 0xff06),
        (0x0b, 0xff07),
        (1, 0xff08),
    ] {
        let reply = HistoryResponse::Failure {
            opcode: 4,
            status,
            reason,
        };
        let mut out = [0; 4];
        assert_eq!(reply.encode(&mut out), Ok(4));
        assert_eq!(HistoryResponse::decode(4, &out), Ok(reply));
    }
}

#[test]
fn all_eighteen_frozen_history_examples_decode_and_reencode_exactly() {
    let mut count = 0;
    for line in include_str!("fixtures/wap12.txt").lines() {
        let Some(meta) = line.strip_prefix("# 10.") else {
            continue;
        };
        let (opcode, rest) = meta.split_once(" kind=").unwrap();
        let (kind, hex) = rest.split_once(" expected=").unwrap();
        let opcode = u8::from_str_radix(opcode, 16).unwrap();
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let mut out = [0; 512];
        let n = match kind {
            "0" => HistoryRequest::decode(opcode, &bytes)
                .unwrap()
                .encode(&mut out)
                .unwrap(),
            "1" => HistoryResponse::decode(opcode, &bytes)
                .unwrap()
                .encode(&mut out)
                .unwrap(),
            "2" if opcode == 0x80 => HistoryRecord::decode(&bytes, 128)
                .unwrap()
                .encode(&mut out, 128)
                .unwrap(),
            "2" if opcode == 0x81 => HistoryDone::decode(&bytes)
                .unwrap()
                .encode(&mut out)
                .unwrap(),
            _ => panic!("unexpected fixture metadata"),
        };
        assert_eq!(&out[..n], bytes);
        count += 1;
    }
    assert_eq!(count, 18);
}

#[test]
fn test_should_encode_record_envelope_without_copying_payload() {
    let record = HistoryRecord::decode(&RECORD, 512).unwrap();
    let (header, crc) = record.envelope(512).unwrap();
    assert_eq!(&header, &RECORD[..34]);
    assert_eq!(&crc, &RECORD[38..]);
    assert!(record.envelope(127).is_err());
    let long = HistoryRecord {
        payload: &[0x7b; 474],
        ..record
    };
    let (header, crc) = long.envelope(512).unwrap();
    assert_eq!(&header[32..], &[0xda, 1]);
    let mut bytes = [0; 512];
    bytes[..34].copy_from_slice(&header);
    bytes[34..508].fill(0x7b);
    bytes[508..].copy_from_slice(&crc);
    assert_eq!(HistoryRecord::decode(&bytes, 512).unwrap(), long);
    assert!(long.envelope(511).is_err());
}

#[test]
fn test_should_validate_filter_without_encoding_a_request() {
    let mut filter = device_protocol::HistoryFilter {
        domain: 1,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    };
    assert_eq!(filter.validate(true), Ok(()));
    filter.filter_flags = 2;
    filter.record_type = 1;
    assert_eq!(filter.validate(true), Ok(()));
    assert!(filter.validate(false).is_err());
    filter.filter_flags = 0;
    assert!(filter.validate(true).is_err());
}

#[test]
fn test_should_validate_snapshot_without_response_staging() {
    let mut snapshot = device_protocol::HistorySnapshot {
        snapshot: 1,
        epoch: 1,
        first_id: 4,
        last_id: 4,
        count: 1,
        lease_s: 60,
    };
    assert_eq!(snapshot.validate(), Ok(()));
    snapshot.count = 0;
    assert!(snapshot.validate().is_err());
    snapshot.count = 1;
    snapshot.snapshot = 0;
    assert!(snapshot.validate().is_err());
}
