//! Four independent register reads form one complete touch frame.
use device_touch::read_frame;

#[test]
fn reads_one_byte_at_a_time_in_supplied_order() {
    let mut calls = Vec::new();
    let frame = read_frame(0x15, [1, 4, 6, 2], |address, register, out| {
        calls.push((address, register, out.len()));
        out[0] = register + 10;
        Ok::<(), ()>(())
    });
    assert_eq!(frame, Ok([11, 14, 16, 12]));
    assert_eq!(
        calls,
        [(0x15, 1, 1), (0x15, 4, 1), (0x15, 6, 1), (0x15, 2, 1)]
    );
}

#[test]
fn forwards_address_and_repeated_registers_without_policy() {
    let mut calls = 0;
    let frame = read_frame(255, [8; 4], |address, register, out| {
        assert_eq!((address, register, out.len()), (255, 8, 1));
        calls += 1;
        out[0] = calls;
        Ok::<(), ()>(())
    });
    assert_eq!(frame, Ok([1, 2, 3, 4]));
}

#[test]
fn stops_on_each_underlying_failure_and_exposes_no_partial_frame() {
    for fail_at in 0..4 {
        let mut calls = 0;
        let result = read_frame(0x15, [1, 4, 6, 2], |_, _, out| {
            out[0] = 99;
            calls += 1;
            if calls == fail_at + 1 {
                Err(fail_at)
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Err(fail_at));
        assert_eq!(calls, fail_at + 1);
    }
}
