//! Read-only access to Microsoft Compound File Binary files.
//!
//! Written from Microsoft's published specification, [MS-CFB]: Compound File Binary File
//! Format. Only what is needed to read a named stream is implemented. Every chain and
//! table is bounds-checked, so a damaged file gives an error, never a panic or a hang.

use std::collections::HashSet;

pub const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
const FREESECT: u32 = 0xFFFF_FFFF;
const NOSTREAM: u32 = 0xFFFF_FFFF;
const HEADER_DIFAT: usize = 109;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfbError(pub String);

impl std::fmt::Display for CfbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "compound file: {}", self.0)
    }
}

impl std::error::Error for CfbError {}

fn err<T>(s: impl Into<String>) -> Result<T, CfbError> {
    Err(CfbError(s.into()))
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
}

const KIND_STORAGE: u8 = 1;
const KIND_STREAM: u8 = 2;
const KIND_ROOT: u8 = 5;

/// A compound file held in memory.
pub struct Cfb<'a> {
    data: &'a [u8],
    sector_size: usize,
    mini_sector_size: usize,
    mini_cutoff: u64,
    fat: Vec<u32>,
    minifat: Vec<u32>,
    entries: Vec<Entry>,
    mini_stream: Vec<u8>,
}

pub fn is_cfb(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..8] == SIGNATURE
}

impl<'a> Cfb<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Cfb<'a>, CfbError> {
        if data.len() < 512 || !is_cfb(data) {
            return err("not a compound file");
        }
        if u16_at(data, 28) != 0xFFFE {
            return err("bad byte order mark");
        }
        let shift = u16_at(data, 30);
        let sector_size = match (u16_at(data, 26), shift) {
            (3, 9) => 512,
            (4, 12) => 4096,
            (v, s) => return err(format!("unsupported version {v} with sector shift {s}")),
        };
        let mini_shift = u16_at(data, 32);
        if mini_shift != 6 {
            return err(format!("unsupported mini sector shift {mini_shift}"));
        }
        let num_fat = u32_at(data, 44) as usize;
        let first_dir = u32_at(data, 48);
        let mini_cutoff = u32_at(data, 56) as u64;
        let first_minifat = u32_at(data, 60);
        let first_difat = u32_at(data, 68);
        let num_difat = u32_at(data, 72) as usize;

        let mut cfb = Cfb {
            data,
            sector_size,
            mini_sector_size: 64,
            mini_cutoff,
            fat: Vec::new(),
            minifat: Vec::new(),
            entries: Vec::new(),
            mini_stream: Vec::new(),
        };
        let total_sectors = data.len().saturating_sub(sector_size) / sector_size + 1;
        if num_fat > total_sectors {
            return err("more FAT sectors than the file holds");
        }

        // DIFAT: the first 109 FAT sector numbers are in the header, the rest in a chain.
        let mut fat_sectors: Vec<u32> = (0..HEADER_DIFAT).map(|i| u32_at(data, 76 + 4 * i)).collect();
        let per_difat = sector_size / 4 - 1;
        let mut next = first_difat;
        let mut seen = HashSet::new();
        for _ in 0..num_difat {
            if next == ENDOFCHAIN || next == FREESECT {
                break;
            }
            if !seen.insert(next) {
                return err("DIFAT chain loops");
            }
            let s = cfb.sector(next)?;
            fat_sectors.extend((0..per_difat).map(|i| u32_at(s, 4 * i)));
            next = u32_at(s, 4 * per_difat);
        }
        fat_sectors.truncate(num_fat);
        if fat_sectors.len() < num_fat {
            return err("DIFAT lists fewer FAT sectors than the header");
        }
        let mut fat = Vec::with_capacity(num_fat * sector_size / 4);
        for &fs in &fat_sectors {
            let s = cfb.sector(fs)?;
            fat.extend((0..sector_size / 4).map(|i| u32_at(s, 4 * i)));
        }
        cfb.fat = fat;

        let dir = cfb.read_chain(first_dir, None)?;
        cfb.entries = dir.chunks_exact(128).map(parse_entry).collect();
        let root = match cfb.entries.first() {
            Some(e) if e.kind == KIND_ROOT => e.clone(),
            _ => return err("missing root entry"),
        };

        if first_minifat != ENDOFCHAIN && first_minifat != FREESECT {
            let mf = cfb.read_chain(first_minifat, None)?;
            cfb.minifat = mf.chunks_exact(4).map(|c| u32_at(c, 0)).collect();
        }
        if root.size > 0 {
            cfb.mini_stream = cfb.read_chain(root.start, Some(root.size))?;
        }
        Ok(cfb)
    }

    /// A whole sector, for tables.
    fn sector(&self, n: u32) -> Result<&'a [u8], CfbError> {
        match (n as usize + 1).checked_mul(self.sector_size) {
            Some(s) if s + self.sector_size <= self.data.len() => Ok(&self.data[s..s + self.sector_size]),
            _ => err(format!("sector {n} is past the end of the file")),
        }
    }

    /// A sector of stream data. Some writers end the file part-way through the last one.
    fn data_sector(&self, n: u32) -> Result<&'a [u8], CfbError> {
        match (n as usize + 1).checked_mul(self.sector_size) {
            Some(s) if s < self.data.len() => Ok(&self.data[s..(s + self.sector_size).min(self.data.len())]),
            _ => err(format!("sector {n} is past the end of the file")),
        }
    }

    /// Follows a FAT chain. With `size`, stops once that many bytes are read.
    fn read_chain(&self, start: u32, size: Option<u64>) -> Result<Vec<u8>, CfbError> {
        let mut out = Vec::new();
        let mut n = start;
        let mut seen = vec![false; self.fat.len()];
        while n != ENDOFCHAIN {
            if size.is_some_and(|s| out.len() as u64 >= s) {
                break;
            }
            match seen.get_mut(n as usize) {
                Some(true) => return err("sector chain loops"),
                Some(v) => *v = true,
                None => return err(format!("sector {n} has no FAT entry")),
            }
            out.extend_from_slice(self.data_sector(n)?);
            n = *self.fat.get(n as usize).ok_or_else(|| CfbError(format!("sector {n} has no FAT entry")))?;
        }
        if let Some(s) = size {
            if (out.len() as u64) < s {
                return err("stream is shorter than its directory entry says");
            }
            out.truncate(s as usize);
        }
        Ok(out)
    }

    fn read_mini_chain(&self, start: u32, size: u64) -> Result<Vec<u8>, CfbError> {
        let mut out = Vec::new();
        let mut n = start;
        let mut seen = vec![false; self.minifat.len()];
        let ms = self.mini_sector_size;
        while (out.len() as u64) < size {
            if n == ENDOFCHAIN {
                return err("mini stream chain ends early");
            }
            match seen.get_mut(n as usize) {
                Some(true) => return err("mini sector chain loops"),
                Some(v) => *v = true,
                None => return err(format!("mini sector {n} has no entry")),
            }
            let o = n as usize * ms;
            let chunk = self
                .mini_stream
                .get(o..o + ms)
                .ok_or_else(|| CfbError(format!("mini sector {n} is past the mini stream")))?;
            out.extend_from_slice(chunk);
            n = *self
                .minifat
                .get(n as usize)
                .ok_or_else(|| CfbError(format!("mini sector {n} has no entry")))?;
        }
        out.truncate(size as usize);
        Ok(out)
    }

    /// Children of a storage, in directory order.
    fn children(&self, storage: usize) -> Result<Vec<usize>, CfbError> {
        let mut out = Vec::new();
        let mut stack = vec![self.entries[storage].child];
        let mut seen = HashSet::new();
        while let Some(i) = stack.pop() {
            if i == NOSTREAM {
                continue;
            }
            if !seen.insert(i) {
                return err("directory tree loops");
            }
            let e =
                self.entries.get(i as usize).ok_or_else(|| CfbError(format!("bad directory index {i}")))?;
            out.push(i as usize);
            stack.push(e.left);
            stack.push(e.right);
        }
        Ok(out)
    }

    /// Names of the streams directly under the root storage.
    pub fn root_streams(&self) -> Vec<String> {
        self.children(0)
            .unwrap_or_default()
            .into_iter()
            .filter(|&i| self.entries[i].kind == KIND_STREAM)
            .map(|i| self.entries[i].name.clone())
            .collect()
    }

    /// Reads a stream by path from the root, for example `["\u{5}SummaryInformation"]`.
    /// Names compare case-insensitively, as the specification requires.
    pub fn stream(&self, path: &[&str]) -> Result<Option<Vec<u8>>, CfbError> {
        let mut at = 0usize;
        for (depth, part) in path.iter().enumerate() {
            let last = depth + 1 == path.len();
            let found = self.children(at)?.into_iter().find(|&i| {
                let e = &self.entries[i];
                let kind_ok = if last { e.kind == KIND_STREAM } else { e.kind == KIND_STORAGE };
                kind_ok && e.name.to_lowercase() == part.to_lowercase()
            });
            match found {
                Some(i) => at = i,
                None => return Ok(None),
            }
        }
        let e = &self.entries[at];
        let size = if self.sector_size == 512 { e.size & 0xFFFF_FFFF } else { e.size };
        if size < self.mini_cutoff {
            self.read_mini_chain(e.start, size).map(Some)
        } else {
            self.read_chain(e.start, Some(size)).map(Some)
        }
    }
}

fn parse_entry(b: &[u8]) -> Entry {
    let name_len = (u16_at(b, 64) as usize).min(64);
    let units: Vec<u16> = (0..name_len / 2).map(|i| u16_at(b, 2 * i)).take_while(|&u| u != 0).collect();
    Entry {
        name: String::from_utf16_lossy(&units),
        kind: b[66],
        left: u32_at(b, 68),
        right: u32_at(b, 72),
        child: u32_at(b, 76),
        start: u32_at(b, 116),
        size: u64::from_le_bytes(b[120..128].try_into().unwrap()),
    }
}

/// Builds small version 3 compound files for tests.
#[cfg(any(test, feature = "fixtures"))]
pub mod writer {
    const SS: usize = 512;

    struct Alloc {
        sectors: Vec<Vec<u8>>,
        fat: Vec<u32>,
    }

    impl Alloc {
        fn chain(&mut self, data: &[u8]) -> u32 {
            if data.is_empty() {
                return super::ENDOFCHAIN;
            }
            let start = self.sectors.len() as u32;
            let n = data.len().div_ceil(SS);
            for i in 0..n {
                let mut s = data[i * SS..((i + 1) * SS).min(data.len())].to_vec();
                s.resize(SS, 0);
                self.sectors.push(s);
                self.fat.push(if i + 1 == n { super::ENDOFCHAIN } else { start + i as u32 + 1 });
            }
            start
        }
    }

    fn entry(name: &str, kind: u8, right: u32, child: u32, start: u32, size: u64) -> Vec<u8> {
        let mut e = vec![0u8; 128];
        let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
        for (i, u) in units.iter().enumerate() {
            e[2 * i..2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
        e[64..66].copy_from_slice(&((units.len() * 2) as u16).to_le_bytes());
        e[66] = kind;
        e[67] = 1; // black
        e[68..72].copy_from_slice(&super::NOSTREAM.to_le_bytes());
        e[72..76].copy_from_slice(&right.to_le_bytes());
        e[76..80].copy_from_slice(&child.to_le_bytes());
        e[116..120].copy_from_slice(&start.to_le_bytes());
        e[120..128].copy_from_slice(&size.to_le_bytes());
        e
    }

    /// A compound file whose root holds the given streams. Streams under 4096 bytes go in
    /// the mini stream, as real writers do.
    pub fn build(streams: &[(&str, &[u8])]) -> Vec<u8> {
        let mut a = Alloc { sectors: Vec::new(), fat: Vec::new() };
        let mut mini = Vec::new();
        let mut minifat: Vec<u32> = Vec::new();
        let mut starts = Vec::new();
        for (_, data) in streams {
            if data.len() < 4096 {
                let start = (mini.len() / 64) as u32;
                let n = data.len().div_ceil(64).max(1);
                for i in 0..n {
                    minifat.push(if i + 1 == n { super::ENDOFCHAIN } else { start + i as u32 + 1 });
                }
                mini.extend_from_slice(data);
                mini.resize((start as usize + n) * 64, 0);
                starts.push(start);
            } else {
                starts.push(a.chain(data));
            }
        }
        let mini_start = a.chain(&mini);
        let mf_bytes: Vec<u8> = minifat.iter().flat_map(|v| v.to_le_bytes()).collect();
        let minifat_start = a.chain(&mf_bytes);

        let mut dir =
            entry("Root Entry", super::KIND_ROOT, super::NOSTREAM, 1, mini_start, mini.len() as u64);
        for (i, (name, data)) in streams.iter().enumerate() {
            let right = if i + 1 < streams.len() { i as u32 + 2 } else { super::NOSTREAM };
            dir.extend(entry(name, super::KIND_STREAM, right, super::NOSTREAM, starts[i], data.len() as u64));
        }
        let dir_start = a.chain(&dir);

        // The FAT covers itself too.
        let mut num_fat = 1;
        while (a.sectors.len() + num_fat) > num_fat * (SS / 4) {
            num_fat += 1;
        }
        assert!(num_fat <= super::HEADER_DIFAT);
        let fat_first = a.sectors.len() as u32;
        for _ in 0..num_fat {
            a.fat.push(0xFFFF_FFFD);
            a.sectors.push(vec![0; SS]);
        }
        let mut fat = a.fat.clone();
        fat.resize(num_fat * SS / 4, super::FREESECT);
        for (i, chunk) in fat.chunks(SS / 4).enumerate() {
            let bytes: Vec<u8> = chunk.iter().flat_map(|v| v.to_le_bytes()).collect();
            a.sectors[fat_first as usize + i] = bytes;
        }

        let mut h = vec![0u8; SS];
        h[..8].copy_from_slice(&super::SIGNATURE);
        h[24..26].copy_from_slice(&0x3Eu16.to_le_bytes());
        h[26..28].copy_from_slice(&3u16.to_le_bytes());
        h[28..30].copy_from_slice(&0xFFFEu16.to_le_bytes());
        h[30..32].copy_from_slice(&9u16.to_le_bytes());
        h[32..34].copy_from_slice(&6u16.to_le_bytes());
        h[44..48].copy_from_slice(&(num_fat as u32).to_le_bytes());
        h[48..52].copy_from_slice(&dir_start.to_le_bytes());
        h[56..60].copy_from_slice(&4096u32.to_le_bytes());
        h[60..64].copy_from_slice(&minifat_start.to_le_bytes());
        h[64..68].copy_from_slice(&(mf_bytes.len().div_ceil(SS) as u32).to_le_bytes());
        h[68..72].copy_from_slice(&super::ENDOFCHAIN.to_le_bytes());
        for i in 0..super::HEADER_DIFAT {
            let v = if i < num_fat { fat_first + i as u32 } else { super::FREESECT };
            h[76 + 4 * i..80 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        let mut out = h;
        for s in a.sectors {
            out.extend(s);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mini_and_regular_streams() {
        let small = b"hello compound file".to_vec();
        let big: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let bytes = writer::build(&[("Small", &small), ("Big", &big), ("Empty", b"")]);
        let cfb = Cfb::parse(&bytes).unwrap();
        assert_eq!(cfb.stream(&["small"]).unwrap().as_deref(), Some(&small[..]));
        assert_eq!(cfb.stream(&["Big"]).unwrap().as_deref(), Some(&big[..]));
        assert_eq!(cfb.stream(&["Empty"]).unwrap().as_deref(), Some(&b""[..]));
        assert_eq!(cfb.stream(&["Missing"]).unwrap(), None);
        let mut names = cfb.root_streams();
        names.sort();
        assert_eq!(names, ["Big", "Empty", "Small"]);
    }

    #[test]
    fn damaged_files_are_errors_not_panics() {
        let big: Vec<u8> = vec![7; 6000];
        let good = writer::build(&[("A", b"abc"), ("B", &big)]);
        assert!(Cfb::parse(&good[..100]).is_err());
        assert!(Cfb::parse(b"not a compound file at all").is_err());
        // Truncate, and corrupt every byte of the header one at a time.
        for cut in [600, 1024, 2000, good.len() - 1] {
            if let Ok(c) = Cfb::parse(&good[..cut]) {
                let _ = c.stream(&["B"]);
            }
        }
        for i in 0..512 {
            let mut bad = good.clone();
            bad[i] ^= 0xFF;
            if let Ok(c) = Cfb::parse(&bad) {
                let _ = c.stream(&["A"]);
                let _ = c.stream(&["B"]);
            }
        }
        // A FAT chain that points at itself.
        let mut looped = good.clone();
        let fat_sector = u32_at(&looped, 76) as usize;
        let o = (fat_sector + 1) * 512;
        let b_start = 0u32; // B is the first regular chain
        looped[o..o + 4].copy_from_slice(&b_start.to_le_bytes());
        let c = Cfb::parse(&looped).unwrap();
        assert!(c.stream(&["B"]).is_err());
    }
}
