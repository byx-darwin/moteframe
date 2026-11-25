//! Restricted-memory RFC1950/RFC1951 decoder. Each stream produces exactly1KiB;
//! distance references must remain inside the current block. No 32KiB window.
use super::{ArchiveError as Error, BLOCK_BYTES};
use device_core::ReadAt;

struct Input<'a, R> {
    source: &'a mut R,
    start: u32,
    len: usize,
    pos: usize,
    cache: [u8; 256],
    bits: u32,
    available: u8,
}
impl<R: ReadAt> Input<'_, R> {
    fn byte(&mut self) -> Result<u8, Error<R::Error>> {
        if self.pos >= self.len {
            return Err(Error::Format);
        }
        let slot = self.pos % 256;
        if slot == 0 {
            let loaded = 256.min(self.len - self.pos);
            self.source
                .read_exact(
                    self.start + u32::try_from(self.pos).map_err(|_| Error::Format)?,
                    &mut self.cache[..loaded],
                )
                .map_err(Error::Source)?;
        }
        let b = self.cache[slot];
        self.pos += 1;
        Ok(b)
    }
    fn bits(&mut self, n: u8) -> Result<u32, Error<R::Error>> {
        while self.available < n {
            self.bits |= u32::from(self.byte()?) << self.available;
            self.available += 8;
        }
        let v = self.bits & ((1_u32 << n) - 1);
        self.bits >>= n;
        self.available -= n;
        Ok(v)
    }
    fn align(&mut self) {
        self.bits = 0;
        self.available = 0;
    }
}
#[derive(Debug)]
struct Huff<const N: usize> {
    count: [u16; 16],
    symbols: [u16; N],
}
impl<const N: usize> Huff<N> {
    fn empty() -> Self {
        Self {
            count: [0; 16],
            symbols: [0; N],
        }
    }
    fn build<E>(&mut self, lengths: &[u8], allow_single: bool) -> Result<(), Error<E>> {
        self.count.fill(0);
        if lengths.len() > N {
            return Err(Error::Format);
        }
        for &len in lengths {
            if len > 15 {
                return Err(Error::Format);
            }
            self.count[usize::from(len)] += 1;
        }
        let used = lengths.len() - usize::from(self.count[0]);
        if used == 0 {
            return Err(Error::Format);
        }
        let mut left = 1_i32;
        for bits in 1..16 {
            left = (left << 1) - i32::from(self.count[bits]);
            if left < 0 {
                return Err(Error::Format);
            }
        }
        if left != 0 && !(allow_single && used == 1 && self.count[1] == 1) {
            return Err(Error::Format);
        }
        let mut offsets = [0_u16; 16];
        for n in 1..15 {
            offsets[n + 1] = offsets[n] + self.count[n];
        }
        for (symbol, &len) in lengths.iter().enumerate() {
            if len != 0 {
                let at = &mut offsets[usize::from(len)];
                self.symbols[usize::from(*at)] =
                    u16::try_from(symbol).map_err(|_| Error::Format)?;
                *at += 1;
            }
        }
        Ok(())
    }
    fn symbol<R: ReadAt>(&self, input: &mut Input<'_, R>) -> Result<u16, Error<R::Error>> {
        let (mut code, mut first, mut index) = (0_u32, 0_u32, 0_usize);
        for len in 1..16 {
            code |= input.bits(1)?;
            let count = u32::from(self.count[len]);
            if code >= first && code - first < count {
                return self
                    .symbols
                    .get(index + usize::try_from(code - first).map_err(|_| Error::Format)?)
                    .copied()
                    .ok_or(Error::Format);
            }
            index += usize::from(self.count[len]);
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(Error::Format)
    }
}
fn tables<R: ReadAt>(
    kind: u32,
    input: &mut Input<'_, R>,
    lit: &mut Huff<288>,
    dist: &mut Huff<32>,
) -> Result<(), Error<R::Error>> {
    let mut lengths = [0_u8; 320];
    if kind == 1 {
        lengths[..144].fill(8);
        lengths[144..256].fill(9);
        lengths[256..280].fill(7);
        lengths[280..288].fill(8);
        lit.build(&lengths[..288], false)?;
        lengths[..32].fill(5);
        return dist.build(&lengths[..32], false);
    }
    let nl = usize::try_from(input.bits(5)?).map_err(|_| Error::Format)? + 257;
    let distance_count = usize::try_from(input.bits(5)?).map_err(|_| Error::Format)? + 1;
    let nc = usize::try_from(input.bits(4)?).map_err(|_| Error::Format)? + 4;
    if nl > 286 || distance_count > 32 {
        return Err(Error::Format);
    }
    let order = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut codes = [0; 19];
    for &index in &order[..nc] {
        codes[index] = u8::try_from(input.bits(3)?).map_err(|_| Error::Format)?;
    }
    let mut code = Huff::<19>::empty();
    code.build(&codes, false)?;
    let mut cursor = 0;
    while cursor < nl + distance_count {
        let symbol = code.symbol(input)?;
        let (value, repeat) = match symbol {
            0..=15 => (u8::try_from(symbol).map_err(|_| Error::Format)?, 1),
            16 => {
                if cursor == 0 {
                    return Err(Error::Format);
                }
                (
                    lengths[cursor - 1],
                    usize::try_from(input.bits(2)?).map_err(|_| Error::Format)? + 3,
                )
            }
            17 => (
                0,
                usize::try_from(input.bits(3)?).map_err(|_| Error::Format)? + 3,
            ),
            18 => (
                0,
                usize::try_from(input.bits(7)?).map_err(|_| Error::Format)? + 11,
            ),
            _ => return Err(Error::Format),
        };
        if repeat > nl + distance_count - cursor {
            return Err(Error::Format);
        }
        lengths[cursor..cursor + repeat].fill(value);
        cursor += repeat;
    }
    if lengths[256] == 0 {
        return Err(Error::Format);
    }
    lit.build(&lengths[..nl], true)?;
    // RFC permits an absent distance tree when no match is emitted. Use an
    // invalid empty tree; symbol() fails if a match tries to use it.
    if lengths[nl..nl + distance_count].iter().all(|x| *x == 0) {
        dist.count.fill(0);
        Ok(())
    } else {
        dist.build(&lengths[nl..nl + distance_count], true)
    }
}
const LENGTH: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEX: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DEX: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
fn stored<R: ReadAt>(
    input: &mut Input<'_, R>,
    out: &mut [u8; BLOCK_BYTES],
    cursor: &mut usize,
) -> Result<(), Error<R::Error>> {
    input.align();
    let a = input.byte()?;
    let b = input.byte()?;
    let n = u16::from_le_bytes([a, b]);
    let a = input.byte()?;
    let b = input.byte()?;
    if u16::from_le_bytes([a, b]) != !n || usize::from(n) > BLOCK_BYTES - *cursor {
        return Err(Error::Format);
    }
    for dest in &mut out[*cursor..*cursor + usize::from(n)] {
        *dest = input.byte()?;
    }
    *cursor += usize::from(n);
    Ok(())
}
pub(super) fn decode<R: ReadAt>(
    source: &mut R,
    start: u32,
    len: u16,
    out: &mut [u8; BLOCK_BYTES],
) -> Result<(), Error<R::Error>> {
    let mut input = Input {
        source,
        start,
        len: usize::from(len),
        pos: 0,
        cache: [0; 256],
        bits: 0,
        available: 0,
    };
    let cmf = input.byte()?;
    let flg = input.byte()?;
    if cmf & 15 != 8
        || cmf >> 4 > 7
        || (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0
        || flg & 32 != 0
    {
        return Err(Error::Format);
    }
    let window = 1_usize << (usize::from(cmf >> 4) + 8);
    let mut cursor = 0;
    let mut lit = Huff::<288>::empty();
    let mut dist = Huff::<32>::empty();
    loop {
        let last = input.bits(1)?;
        let kind = input.bits(2)?;
        match kind {
            0 => stored(&mut input, out, &mut cursor)?,
            1 | 2 => {
                tables(kind, &mut input, &mut lit, &mut dist)?;
                loop {
                    let symbol = lit.symbol(&mut input)?;
                    match symbol {
                        0..=255 => {
                            if cursor >= BLOCK_BYTES {
                                return Err(Error::Format);
                            }
                            out[cursor] = u8::try_from(symbol).map_err(|_| Error::Format)?;
                            cursor += 1;
                        }
                        256 => break,
                        257..=285 => {
                            let index = usize::from(symbol - 257);
                            let n = usize::from(LENGTH[index])
                                + usize::try_from(input.bits(LEX[index])?)
                                    .map_err(|_| Error::Format)?;
                            let d = usize::from(dist.symbol(&mut input)?);
                            if d >= 30 {
                                return Err(Error::Format);
                            }
                            let back = usize::from(DIST[d])
                                + usize::try_from(input.bits(DEX[d])?)
                                    .map_err(|_| Error::Format)?;
                            if back > cursor || back > window || n > BLOCK_BYTES - cursor {
                                return Err(Error::Format);
                            }
                            for _ in 0..n {
                                out[cursor] = out[cursor - back];
                                cursor += 1;
                            }
                        }
                        _ => return Err(Error::Format),
                    }
                }
            }
            _ => return Err(Error::Format),
        }
        if last != 0 {
            break;
        }
    }
    input.align();
    if cursor != BLOCK_BYTES {
        return Err(Error::Format);
    }
    let mut stored = [0; 4];
    for b in &mut stored {
        *b = input.byte()?;
    }
    if input.pos != input.len {
        return Err(Error::Format);
    }
    let (mut a, mut b) = (1_u32, 0_u32);
    for v in out {
        a = (a + u32::from(*v)) % 65521;
        b = (b + a) % 65521;
    }
    if u32::from_be_bytes(stored) != (b << 16) | a {
        return Err(Error::Integrity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Fixture conversions and assertions deliberately panic when a test fails.
    #![allow(clippy::unwrap_used)]
    extern crate std;
    use super::*;
    use std::vec::Vec;
    #[derive(Debug)]
    struct Bytes<'a>(&'a [u8]);
    impl ReadAt for Bytes<'_> {
        type Error = ();
        fn capacity(&self) -> u32 {
            u32::try_from(self.0.len()).unwrap()
        }
        fn read_exact(&mut self, at: u32, out: &mut [u8]) -> Result<(), ()> {
            assert!(out.len() <= 256);
            out.copy_from_slice(self.0.get(at as usize..at as usize + out.len()).ok_or(())?);
            Ok(())
        }
    }
    fn run(packed: &[u8]) -> Result<[u8; 1024], Error<()>> {
        let mut out = [0; 1024];
        decode(
            &mut Bytes(packed),
            0,
            u16::try_from(packed.len()).unwrap(),
            &mut out,
        )?;
        Ok(out)
    }
    const FIXED: &[u8] = include_bytes!("../tests/fixtures/fixed.zlib");
    const STORED: &[u8] = include_bytes!("../tests/fixtures/stored.zlib");
    const DYNAMIC: &[u8] = include_bytes!("../tests/fixtures/dynamic.zlib");
    #[test]
    fn test_should_decode_stored_fixed_dynamic_and_overlapping_distance() {
        for (packed, raw) in [
            (
                FIXED,
                include_bytes!("../tests/fixtures/fixed.raw").as_slice(),
            ),
            (
                STORED,
                include_bytes!("../tests/fixtures/stored.raw").as_slice(),
            ),
            (
                DYNAMIC,
                include_bytes!("../tests/fixtures/dynamic.raw").as_slice(),
            ),
        ] {
            assert_eq!(run(packed).unwrap().as_slice(), raw);
        }
    }
    #[test]
    fn test_should_reject_every_truncation_adler_header_and_trailing_input() {
        for packed in [FIXED, STORED, DYNAMIC] {
            for cut in 0..packed.len() {
                assert!(run(&packed[..cut]).is_err(), "cut{cut}");
            }
            let mut b = packed.to_vec();
            let n = b.len();
            b[n - 1] ^= 1;
            assert_eq!(run(&b), Err(Error::Integrity));
            let mut b = packed.to_vec();
            b.push(0);
            assert!(run(&b).is_err());
        }
        for b in [
            [0x78, 0x20, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0],
            [0x78, 0xda, 7, 0, 0, 0],
        ] {
            assert!(run(&b).is_err());
        }
    }
    #[test]
    fn test_should_enforce_the_declared_zlib_window() {
        let packed = include_bytes!("../tests/fixtures/distance512.zlib");
        assert_eq!(
            &run(packed).unwrap(),
            include_bytes!("../tests/fixtures/distance512.raw")
        );
        let mut small_window = packed.to_vec();
        small_window[0] = 8;
        small_window[1] = 29;
        assert!(matches!(run(&small_window), Err(Error::Format)));
    }
    #[test]
    fn test_should_decode_multiple_deflate_blocks_and_absent_unused_distance_tree() {
        assert_eq!(
            &run(include_bytes!("../tests/fixtures/multiple.zlib")).unwrap(),
            include_bytes!("../tests/fixtures/multiple.raw")
        );
        assert_eq!(
            run(include_bytes!("../tests/fixtures/absent-distance.zlib")).unwrap(),
            [0; 1024]
        );
    }
    fn fixed_stream(symbols: &[u16], distance: Option<u8>) -> Vec<u8> {
        let mut bits = std::vec![1, 1, 0];
        for &symbol in symbols {
            let (code, width) = match symbol {
                0..=143 => (48 + symbol, 8),
                144..=255 => (400 + symbol - 144, 9),
                256..=279 => (symbol - 256, 7),
                _ => (192 + symbol - 280, 8),
            };
            for shift in (0..width).rev() {
                bits.push(u8::try_from((code >> shift) & 1).unwrap());
            }
        }
        if let Some(d) = distance {
            for shift in (0..5).rev() {
                bits.push((d >> shift) & 1);
            }
        }
        let mut result = std::vec![0x78, 0xda];
        for chunk in bits.chunks(8) {
            let mut byte = 0;
            for (shift, bit) in chunk.iter().enumerate() {
                byte |= bit << shift;
            }
            result.push(byte);
        }
        result.extend_from_slice(&[0; 4]);
        result
    }
    #[test]
    fn test_should_reject_out_of_block_distances_reserved_symbols_and_output_overflow() {
        for (symbols, distance) in [
            (&[257][..], Some(0)),
            (&[65, 257][..], Some(1)),
            (&[65, 257][..], Some(30)),
            (&[286][..], None),
            (&[287][..], None),
        ] {
            assert!(matches!(
                run(&fixed_stream(symbols, distance)),
                Err(Error::Format)
            ));
        }
        assert!(matches!(
            run(&fixed_stream(&[65; 1025], None)),
            Err(Error::Format)
        ));
        let mut b = STORED.to_vec();
        b[5] ^= 1;
        assert!(matches!(run(&b), Err(Error::Format)));
    }
    #[test]
    fn test_should_fail_closed_on_malformed_huffman_and_never_panic_on_mutations() {
        let mut table = Huff::<4>::empty();
        assert!(table.build::<()>(&[1, 1, 1], true).is_err());
        assert!(table.build::<()>(&[2, 2], true).is_err());
        assert!(table.build::<()>(&[16], true).is_err());
        assert!(table.build::<()>(&[0, 0], true).is_err());
        assert!(table.build::<()>(&[1], false).is_err());
        for packed in [FIXED, STORED, DYNAMIC] {
            for at in 0..packed.len() {
                for bit in 0..8 {
                    let mut b = packed.to_vec();
                    b[at] ^= 1 << bit;
                    let _ = run(&b);
                }
            }
        }
        for n in 0..1040 {
            let b: Vec<_> = (0..n)
                .map(|k| u8::try_from((k * 31 + n * 17) % 256).unwrap())
                .collect();
            let _ = run(&b);
        }
    }
}
