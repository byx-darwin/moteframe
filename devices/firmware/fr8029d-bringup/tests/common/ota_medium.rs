use fr8029d_bringup::{
    flash_layout::{CONFIG_A, CONFIG_B, FLASH_ENV, STAGING},
    ota::{Error, INFO, MARKER, Medium},
};
pub(super) struct Nor {
    pub(super) bytes: Vec<u8>,
    pub(super) mutations: usize,
    pub(super) corrupt_header: bool,
    pub(super) fail_marker: bool,
}

impl Nor {
    pub(super) fn new() -> Self {
        let mut bytes = vec![0x33; 1 << 20];
        for (range, value) in [(FLASH_ENV, 0x12), (CONFIG_A, 0xab), (CONFIG_B, 0xbc)] {
            bytes[range.start as usize..range.end as usize].fill(value);
        }
        Self {
            bytes,
            mutations: 0,
            corrupt_header: false,
            fail_marker: false,
        }
    }
    pub(super) fn check_neighbors(&self) {
        for (range, value) in [
            (FLASH_ENV, 0x12),
            (CONFIG_A, 0xab),
            (CONFIG_B, 0xbc),
            (STAGING.end..1 << 20, 0x33),
        ] {
            assert!(
                self.bytes[range.start as usize..range.end as usize]
                    .iter()
                    .all(|&b| b == value)
            );
        }
    }
    pub(super) fn word(&self, offset: u32) -> u32 {
        let i = offset as usize;
        u32::from_le_bytes(self.bytes[i..i + 4].try_into().unwrap())
    }
}

impl Medium for &mut Nor {
    fn read(&mut self, offset: u32, dest: &mut [u8]) -> Result<(), Error> {
        dest.copy_from_slice(&self.bytes[offset as usize..offset as usize + dest.len()]);
        Ok(())
    }
    fn erase(&mut self, offset: u32) -> Result<(), Error> {
        assert!(offset == INFO || STAGING.contains(&offset));
        assert!(offset.is_multiple_of(4096));
        self.mutations += 1;
        self.bytes[offset as usize..offset as usize + 4096].fill(0xff);
        Ok(())
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        assert!(offset == INFO || offset == MARKER || STAGING.contains(&offset));
        assert!((offset % 256) as usize + bytes.len() <= 256);
        self.mutations += 1;
        for (i, &byte) in bytes.iter().enumerate() {
            if self.fail_marker && offset == MARKER && i == 2 {
                return Err(Error::Media);
            }
            self.bytes[offset as usize + i] &= byte;
        }
        if self.corrupt_header && offset == INFO {
            self.bytes[INFO as usize + 14] ^= 1;
        }
        Ok(())
    }
}

pub(super) fn crc32(bytes: &[u8]) -> u32 {
    let mut state = u32::MAX;
    for byte in bytes {
        state ^= u32::from(*byte);
        for _ in 0..8 {
            state = if state & 1 == 1 {
                (state >> 1) ^ 0xedb8_8320
            } else {
                state >> 1
            };
        }
    }
    !state
}

pub(super) fn image(size: usize) -> Vec<u8> {
    let mut image: Vec<_> = (0..size).map(|i| u8::try_from(i % 251).unwrap()).collect();
    image[..4].copy_from_slice(&0x2000_a000_u32.to_le_bytes());
    image[4..8].copy_from_slice(&0x1000_2021_u32.to_le_bytes());
    image
}

pub(super) fn begin(length: u32, crc: u32) -> Vec<u8> {
    [&[1][..], &length.to_le_bytes(), &crc.to_le_bytes()].concat()
}
