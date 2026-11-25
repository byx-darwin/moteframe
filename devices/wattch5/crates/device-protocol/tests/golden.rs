//! Protocol boundary regressions.
use device_protocol::{Frame, Progress, Reassembler};
#[test]
fn test_should_reassemble_all_203_document_examples_and_318_frames() {
    let mut groups = 0;
    let mut frames = 0;
    let mut expected = String::new();
    let mut rx = Reassembler::<512>::new(20, 512).unwrap_or_else(|e| panic!("{e:?}"));
    for line in include_str!("fixtures/wap12.txt").lines() {
        if let Some((_, value)) = line.split_once("expected=") {
            expected = value.to_owned();
            rx.reset();
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bytes: Vec<u8> = (0..line.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&line[i..i + 2], 16).unwrap_or_else(|e| panic!("{e:?}")))
            .collect();
        frames += 1;
        if rx
            .push(
                Frame::decode(&bytes, 20, 512).unwrap_or_else(|e| panic!("{e:?}")),
                0,
            )
            .unwrap_or_else(|e| panic!("{e:?}"))
            == Progress::Complete
        {
            let body = rx.message().unwrap_or_else(|e| panic!("{e:?}")).payload;
            let wanted: Vec<u8> = (0..expected.len())
                .step_by(2)
                .map(|i| {
                    u8::from_str_radix(&expected[i..i + 2], 16).unwrap_or_else(|e| panic!("{e:?}"))
                })
                .collect();
            assert_eq!(body, wanted);
            groups += 1;
        }
    }
    assert_eq!(groups, 203);
    assert_eq!(frames, 318);
}
