//! Host NOR and reference copy loop. No target or ROM instructions are executed.
#![cfg(feature = "ota-v1")]
#![allow(
    clippy::unwrap_used,
    reason = "invalid fixture values must fail the test"
)]
use fr8029d_bringup::{
    flash_layout::{APP, COPY_UNIT, STAGING, UpgradePlan},
    ota::{Error, INFO, MAGIC, MARKER, Receiver},
};
use sha2::{Digest, Sha256};

#[path = "common/ota_medium.rs"]
mod ota_medium;
use ota_medium::{Nor, begin, crc32, image};

fn download(nor: &mut Nor, payload: &[u8], digest: [u8; 32]) -> Result<(), Error> {
    let mut trusted = fr8029d_bringup::ota::manifest(payload).unwrap();
    trusted[8..].copy_from_slice(&digest);
    let mut rx = Receiver::new(nor, trusted);
    rx.receive(&begin(
        u32::try_from(payload.len()).unwrap(),
        crc32(payload),
    ))?;
    for (i, chunk) in payload.chunks(15).enumerate() {
        rx.receive(
            &[
                &[2][..],
                &u32::try_from(i * 15).unwrap().to_le_bytes(),
                chunk,
            ]
            .concat(),
        )?;
    }
    rx.receive(&[3])
}

#[test]
fn rejects_before_first_mutation_and_never_publishes_bad_readback() {
    let mut nor = Nor::new();
    {
        let oversized = image(0x3a001);
        let mut rx = Receiver::new(
            &mut nor,
            fr8029d_bringup::ota::manifest(&oversized).unwrap(),
        );
        assert_eq!(
            rx.receive(&begin(0x3a001, crc32(&oversized))),
            Err(Error::Invalid)
        );
        assert_eq!(rx.receive(&[2, 0, 0, 0, 0, 5]), Err(Error::Invalid));
        assert_eq!(rx.receive(&[3]), Err(Error::Invalid));
    }
    assert_eq!(nor.mutations, 0);
    let data = image(257);
    let digest = Sha256::digest(&data).into();
    // Drop the receiver after the first page reached NOR, keep only the medium.
    {
        let mut rx = Receiver::new(&mut nor, fr8029d_bringup::ota::manifest(&data).unwrap());
        rx.receive(&begin(257, crc32(&data))).unwrap();
        rx.receive(&[&[2, 0, 0, 0, 0][..], &data[..256]].concat())
            .unwrap();
    }
    assert_ne!(nor.word(MARKER), MAGIC);
    assert_eq!(download(&mut nor, &data, [0; 32]), Err(Error::Invalid));
    nor.corrupt_header = true;
    assert_eq!(download(&mut nor, &data, digest), Err(Error::Media));
    assert_eq!(nor.word(MARKER), u32::MAX);
    nor.corrupt_header = false;
    nor.fail_marker = true;
    assert_eq!(download(&mut nor, &data, digest), Err(Error::Media));
    assert_ne!(nor.word(MARKER), MAGIC);
    nor.fail_marker = false;
    assert_eq!(download(&mut nor, &data, digest), Ok(()));
    let count = nor.mutations;
    assert_eq!(download(&mut nor, &data, digest), Err(Error::Pending));
    assert_eq!(nor.mutations, count);
    nor.check_neighbors();
}

// A model boot starts from persisted header and staged data every time. Infer the
// first incomplete block from physical bytes, without assuming a ROM flag format.
fn model_boot(nor: &mut Nor, cut: Option<(usize, usize)>) -> bool {
    assert_eq!(nor.word(MARKER), MAGIC);
    assert_eq!(
        crc32(&nor.bytes[INFO as usize..INFO as usize + 44]),
        nor.word(INFO + 44)
    );
    let length = nor.word(INFO + 8);
    let plan = UpgradePlan::check(
        length,
        length as usize,
        nor.word(INFO + 4),
        nor.word(INFO + 12),
        nor.word(INFO + 16),
    )
    .unwrap();
    assert_eq!(
        crc32(&nor.bytes[STAGING.start as usize..STAGING.start as usize + length as usize]),
        nor.word(INFO + 24)
    );
    let unit = COPY_UNIT as usize;
    for (block, offset) in (0..plan.destination().len()).step_by(unit).enumerate() {
        let src = plan.staging().start as usize + offset;
        let dst = plan.destination().start as usize + offset;
        if nor.bytes[dst..dst + unit] == nor.bytes[src..src + unit] {
            continue;
        }
        nor.bytes[dst..dst + unit].fill(0xff);
        if let Some((index, count)) = cut
            && block == index
        {
            nor.bytes.copy_within(src..src + count, dst);
            return false;
        }
        nor.bytes.copy_within(src..src + unit, dst);
    }
    true
}

#[test]
fn maximum_image_survives_repeated_cuts_during_first_middle_and_last_copy() {
    let data = image(APP.len());
    let mut nor = Nor::new();
    download(&mut nor, &data, Sha256::digest(&data).into()).unwrap();
    for cut in [(0, 0), (0, 256), (14, 4096), (28, 0), (28, 8191)] {
        assert!(!model_boot(&mut nor, Some(cut)));
        nor.check_neighbors();
    }
    assert!(model_boot(&mut nor, None));
    assert_eq!(&nor.bytes[APP.start as usize..APP.end as usize], &data);
    nor.check_neighbors();
}
