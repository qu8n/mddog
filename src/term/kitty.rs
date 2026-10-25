use std::ffi::CString;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

use crate::sweep::{self, write_private};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Medium {
    Shm,
    Temp,
    Direct,
}

pub const TILE_BYTES: usize = 8 << 20;

pub fn storage_limit(herdr: bool) -> usize {
    if herdr { 56 << 20 } else { 300 << 20 }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub struct Shm {
    name: String,
    ptr: *mut u8,
    len: usize,
    owned: bool,
}

impl Shm {
    pub fn create(name: String, len: usize) -> Option<Shm> {
        let c = CString::new(name.as_str()).ok()?;
        let fd = unsafe { libc::shm_open(c.as_ptr(), libc::O_CREAT | libc::O_RDWR | libc::O_EXCL, 0o600) };
        if fd < 0 {
            return None;
        }
        let sized = unsafe { libc::ftruncate(fd, len as libc::off_t) } == 0;
        let shm = Shm::map(fd, name, if sized { len } else { 0 }, true);
        if shm.is_none() {
            unsafe { libc::shm_unlink(c.as_ptr()) };
        }
        shm
    }

    pub fn open(name: &str) -> Option<Shm> {
        let c = CString::new(name).ok()?;
        let fd = unsafe { libc::shm_open(c.as_ptr(), libc::O_RDWR) };
        if fd < 0 {
            return None;
        }
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let len = if unsafe { libc::fstat(fd, &mut st) } == 0 { st.st_size as usize } else { 0 };
        Shm::map(fd, name.to_string(), len, false)
    }

    fn map(fd: libc::c_int, name: String, len: usize, owned: bool) -> Option<Shm> {
        let p = if len == 0 { libc::MAP_FAILED } else { unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) } };
        unsafe { libc::close(fd) };
        (p != libc::MAP_FAILED).then(|| Shm { name, ptr: p.cast(), len, owned })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn hand_off(mut self) -> String {
        self.owned = false;
        std::mem::take(&mut self.name)
    }
}

// The mapping belongs to this value alone, like a Box<[u8]>, so another thread may own it.
unsafe impl Send for Shm {}

impl std::ops::Deref for Shm {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl std::ops::DerefMut for Shm {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr.cast(), self.len) };
        if self.owned {
            shm_unlink(&self.name);
        }
    }
}

pub fn shm_name() -> String {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    format!("/mddog-{:08x}-{}", crate::sweep::nonce() as u32, SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

pub fn shm_write(name: &str, data: &[u8]) -> bool {
    Shm::create(name.to_string(), data.len())
        .map(|mut m| {
            m.copy_from_slice(data);
            m.hand_off();
        })
        .is_some()
}

pub fn shm_unlink(name: &str) {
    if let Ok(c) = CString::new(name) {
        unsafe { libc::shm_unlink(c.as_ptr()) };
    }
}

pub fn temp_path(tag: &str) -> String {
    sweep::temp_path("tty-graphics-protocol-mddog-", &format!("-{tag}")).to_string_lossy().into_owned()
}

pub fn transmit(out: &mut Vec<u8>, medium: Medium, id: u32, width: usize, height: usize, rgba: Shm, seq: u64) -> Medium {
    if medium == Medium::Shm {
        let len = rgba.len();
        let _ = write!(out, "\x1b_Ga=t,f=32,s={width},v={height},t=s,S={len},i={id},q=2;{}\x1b\\", base64(rgba.hand_off().as_bytes()));
        return Medium::Shm;
    }
    if matches!(medium, Medium::Shm | Medium::Temp) {
        let path = temp_path(&seq.to_string());
        if write_private(Path::new(&path), &rgba).is_ok() {
            let _ = write!(out, "\x1b_Ga=t,f=32,s={width},v={height},t=t,i={id},q=2;{}\x1b\\", base64(path.as_bytes()));
            return Medium::Temp;
        }
    }
    let png = png(width, height, &rgba);
    let encoded = base64(png.as_deref().unwrap_or(&rgba));
    let format = if png.is_some() { "f=100".to_string() } else { format!("f=32,s={width},v={height}") };
    let chunks: Vec<&[u8]> = encoded.as_bytes().chunks(4096).collect();
    for (i, c) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        let chunk = std::str::from_utf8(c).unwrap_or("");
        if i == 0 {
            let _ = write!(out, "\x1b_Ga=t,{format},t=d,i={id},q=2,m={more};{chunk}\x1b\\");
        } else {
            let _ = write!(out, "\x1b_Gm={more};{chunk}\x1b\\");
        }
    }
    Medium::Direct
}

fn png(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, u32::try_from(width).ok()?, u32::try_from(height).ok()?);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().ok()?.write_image_data(rgba).ok()?;
    Some(out)
}

pub struct Place {
    pub id: u32,
    pub row: u32,
    pub col: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub cols: u32,
    pub rows: u32,
    pub z: u32,
}

pub fn place(out: &mut String, p: &Place) {
    let _ = write!(out, "\x1b[{};{}H\x1b_Ga=p,i={},p=1,x=0,y={},w={},h={},c={},r={},C=1,", p.row + 1, p.col + 1, p.id, p.y, p.width, p.height, p.cols, p.rows);
    if p.z != 0 {
        let _ = write!(out, "z={},", p.z);
    }
    out.push_str("q=2\x1b\\");
}

pub fn unplace(out: &mut String, id: u32) {
    let _ = write!(out, "\x1b_Ga=d,d=i,i={id},p=1,q=2\x1b\\");
}

pub fn free(out: &mut String, id: u32) {
    let _ = write!(out, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn direct_sends_png() {
        let rgba: Vec<u8> = (0..3 * 2 * 4).map(|i| i as u8 * 10).collect();
        let mut shm = Shm::create(shm_name(), rgba.len()).unwrap();
        shm.copy_from_slice(&rgba);
        let name = shm.name().to_string();
        let mut out = Vec::new();
        assert_eq!(transmit(&mut out, Medium::Direct, 7, 3, 2, shm, 0), Medium::Direct);
        assert!(Shm::open(&name).is_none(), "an object that was not handed off is unlinked");
        let png = png(3, 2, &rgba).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), format!("\x1b_Ga=t,f=100,t=d,i=7,q=2,m=0;{}\x1b\\", base64(&png)));
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut back = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut back).unwrap();
        assert_eq!(back, rgba);
    }

    #[test]
    fn placement_uses_explicit_size_and_cursor() {
        let mut s = String::new();
        place(&mut s, &Place { id: 7, row: 2, col: 0, y: 38, width: 1292, height: 76, cols: 68, rows: 2, z: 0 });
        assert_eq!(s, "\x1b[3;1H\x1b_Ga=p,i=7,p=1,x=0,y=38,w=1292,h=76,c=68,r=2,C=1,q=2\x1b\\");
        s.clear();
        place(&mut s, &Place { id: 8, row: 2, col: 0, y: 0, width: 1292, height: 76, cols: 68, rows: 2, z: 1 });
        assert_eq!(s, "\x1b[3;1H\x1b_Ga=p,i=8,p=1,x=0,y=0,w=1292,h=76,c=68,r=2,C=1,z=1,q=2\x1b\\");
    }

    #[test]
    fn unplace_names_the_placement_so_the_image_stays() {
        // iTerm2 frees the image on d=i without p, as if it were d=I.
        let mut s = String::new();
        unplace(&mut s, 7);
        assert_eq!(s, "\x1b_Ga=d,d=i,i=7,p=1,q=2\x1b\\");
    }

    #[test]
    fn direct_splits_a_large_png_into_chunks() {
        let mut x = 0x2545_f491u32;
        let rgba: Vec<u8> = (0..64 * 64 * 4)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        let mut shm = Shm::create(shm_name(), rgba.len()).unwrap();
        shm.copy_from_slice(&rgba);
        let mut out = Vec::new();
        assert_eq!(transmit(&mut out, Medium::Direct, 9, 64, 64, shm, 0), Medium::Direct);
        let out = String::from_utf8(out).unwrap();
        let chunks: Vec<(&str, &str)> = out.split("\x1b\\").filter(|c| !c.is_empty()).map(|c| c.strip_prefix("\x1b_G").unwrap().split_once(';').unwrap()).collect();
        assert!(chunks.len() > 2, "{}", chunks.len());
        let mut payload = String::new();
        for (i, (keys, data)) in chunks.iter().enumerate() {
            let last = i + 1 == chunks.len();
            assert_eq!(keys.contains("m=0"), last, "chunk {i}: {keys}");
            assert_eq!(keys.contains("m=1"), !last, "chunk {i}: {keys}");
            assert_eq!(keys.contains("a=t") && keys.contains("f=100"), i == 0, "chunk {i}: {keys}");
            assert!(data.len() <= 4096, "chunk {i}: {}", data.len());
            payload.push_str(data);
        }
        assert_eq!(payload, base64(&png(64, 64, &rgba).unwrap()));
        let bytes = unbase64(&payload);
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut back = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut back).unwrap();
        assert_eq!(back, rgba);
    }

    fn unbase64(s: &str) -> Vec<u8> {
        let val = |c: u8| B64.iter().position(|&b| b == c).unwrap() as u32;
        s.as_bytes()
            .chunks(4)
            .flat_map(|q| {
                let n = q.iter().fold(0u32, |n, &c| (n << 6) | if c == b'=' { 0 } else { val(c) });
                let keep = 3 - q.iter().filter(|&&c| c == b'=').count();
                n.to_be_bytes()[1..1 + keep].to_vec()
            })
            .collect::<Vec<u8>>()
    }
}
