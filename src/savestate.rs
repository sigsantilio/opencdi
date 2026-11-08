// Savestates: a byte buffer mirroring the machine's internal state (no files, no threads).
use std::collections::VecDeque;
use std::fmt;

pub const MAGIC: [u8; 8] = *b"OPENCDI\x1a";
// Bump whenever a saved field is added, removed, reordered or changes type.
pub const VERSION: u32 = 10;
pub const HEADER_LEN: usize = 64;
const CPU_BLOB_MAX: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    TooShort,
    BadMagic,
    Version(u32),
    Checksum,
    BiosMismatch,
    DiscMismatch,
    DvcMismatch,
    Truncated,
    Invalid(&'static str),
    Trailing(usize),
    Cpu(String),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::TooShort => write!(f, "buffer shorter than the state it should hold"),
            StateError::BadMagic => write!(f, "not an OpenCDI state (bad magic)"),
            StateError::Version(v) => write!(f, "state version {} not supported (this build reads {})", v, VERSION),
            StateError::Checksum => write!(f, "state checksum mismatch (corrupt or truncated)"),
            StateError::BiosMismatch => write!(f, "state was saved with a different BIOS"),
            StateError::DiscMismatch => write!(f, "state was saved with a different disc"),
            StateError::DvcMismatch => write!(f, "state was saved with a different DVC cartridge setup"),
            StateError::Truncated => write!(f, "state payload ends early"),
            StateError::Invalid(what) => write!(f, "invalid value in state: {}", what),
            StateError::Trailing(n) => write!(f, "{} unread bytes after the state payload", n),
            StateError::Cpu(e) => write!(f, "CPU state: {}", e),
        }
    }
}

impl std::error::Error for StateError {}

pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len()
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], StateError> {
        if n > self.buf.len() {
            return Err(StateError::Truncated);
        }
        let (head, tail) = self.buf.split_at(n);
        self.buf = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], StateError> {
        Ok(self.take(N)?.try_into().unwrap_or([0; N]))
    }

    fn len(&mut self) -> Result<usize, StateError> {
        let n = u64::from_le_bytes(self.array()?);
        // Every saved element takes at least one byte, so a longer count is corrupt.
        if n > self.remaining() as u64 {
            return Err(StateError::Invalid("collection length"));
        }
        Ok(n as usize)
    }
}

// Saved in field order; `load` overwrites in place so host-only fields stay untouched.
pub trait State {
    fn save(&self, w: &mut Vec<u8>);
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError>;

    fn save_slice(s: &[Self], w: &mut Vec<u8>)
    where
        Self: Sized,
    {
        for x in s {
            x.save(w);
        }
    }

    fn load_slice(s: &mut [Self], r: &mut Reader) -> Result<(), StateError>
    where
        Self: Sized,
    {
        for x in s {
            x.load(r)?;
        }
        Ok(())
    }
}

macro_rules! state_int {
    ($($t:ty),*) => {$(
        impl State for $t {
            fn save(&self, w: &mut Vec<u8>) {
                w.extend_from_slice(&self.to_le_bytes());
            }
            fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
                *self = <$t>::from_le_bytes(r.array()?);
                Ok(())
            }
            fn save_slice(s: &[Self], w: &mut Vec<u8>) {
                w.reserve(s.len() * size_of::<$t>());
                for x in s {
                    w.extend_from_slice(&x.to_le_bytes());
                }
            }
            fn load_slice(s: &mut [Self], r: &mut Reader) -> Result<(), StateError> {
                let bytes = r.take(s.len() * size_of::<$t>())?;
                for (x, c) in s.iter_mut().zip(bytes.chunks_exact(size_of::<$t>())) {
                    *x = <$t>::from_le_bytes(c.try_into().unwrap_or_default());
                }
                Ok(())
            }
        }
    )*};
}

state_int!(u16, i16, u32, i32, u64, i64, f32, f64);

impl State for u8 {
    fn save(&self, w: &mut Vec<u8>) {
        w.push(*self);
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        *self = r.take(1)?[0];
        Ok(())
    }
    fn save_slice(s: &[Self], w: &mut Vec<u8>) {
        w.extend_from_slice(s);
    }
    fn load_slice(s: &mut [Self], r: &mut Reader) -> Result<(), StateError> {
        s.copy_from_slice(r.take(s.len())?);
        Ok(())
    }
}

impl State for bool {
    fn save(&self, w: &mut Vec<u8>) {
        w.push(*self as u8);
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        *self = match r.take(1)?[0] {
            0 => false,
            1 => true,
            _ => return Err(StateError::Invalid("bool")),
        };
        Ok(())
    }
}

impl State for usize {
    fn save(&self, w: &mut Vec<u8>) {
        (*self as u64).save(w);
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        let mut v = 0u64;
        v.load(r)?;
        *self = usize::try_from(v).map_err(|_| StateError::Invalid("usize"))?;
        Ok(())
    }
}

impl<T: State, const N: usize> State for [T; N] {
    fn save(&self, w: &mut Vec<u8>) {
        T::save_slice(self, w);
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        T::load_slice(self, r)
    }
}

impl<T: State> State for Box<T> {
    fn save(&self, w: &mut Vec<u8>) {
        (**self).save(w);
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        (**self).load(r)
    }
}

impl<A: State, B: State> State for (A, B) {
    fn save(&self, w: &mut Vec<u8>) {
        self.0.save(w);
        self.1.save(w);
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        self.0.load(r)?;
        self.1.load(r)
    }
}

impl<A: State, B: State, C: State> State for (A, B, C) {
    fn save(&self, w: &mut Vec<u8>) {
        self.0.save(w);
        self.1.save(w);
        self.2.save(w);
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        self.0.load(r)?;
        self.1.load(r)?;
        self.2.load(r)
    }
}

impl<T: State + Default> State for Option<T> {
    fn save(&self, w: &mut Vec<u8>) {
        match self {
            None => w.push(0),
            Some(v) => {
                w.push(1);
                v.save(w);
            }
        }
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        match r.take(1)?[0] {
            0 => *self = None,
            1 => self.get_or_insert_with(T::default).load(r)?,
            _ => return Err(StateError::Invalid("option tag")),
        }
        Ok(())
    }
}

impl<T: State + Default> State for Vec<T> {
    fn save(&self, w: &mut Vec<u8>) {
        (self.len() as u64).save(w);
        T::save_slice(self, w);
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        let n = r.len()?;
        self.clear();
        self.resize_with(n, T::default);
        T::load_slice(self, r)
    }
}

impl<T: State + Default> State for VecDeque<T> {
    fn save(&self, w: &mut Vec<u8>) {
        (self.len() as u64).save(w);
        let (a, b) = self.as_slices();
        T::save_slice(a, w);
        T::save_slice(b, w);
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        let n = r.len()?;
        self.clear();
        self.resize_with(n, T::default);
        self.make_contiguous();
        T::load_slice(self.as_mut_slices().0, r)
    }
}

// Saves the listed fields in order; the destructuring is exhaustive, so a new field fails to build until listed or skipped.
#[macro_export]
macro_rules! impl_state {
    ($ty:ty { $($f:ident),* $(,)? } skip { $($s:ident),* $(,)? }) => {
        impl $crate::savestate::State for $ty {
            fn save(&self, w: &mut Vec<u8>) {
                let Self { $($f,)* $($s: _,)* .. } = self;
                $( $crate::savestate::State::save($f, w); )*
            }
            fn load(&mut self, r: &mut $crate::savestate::Reader) -> Result<(), $crate::savestate::StateError> {
                let Self { $($f,)* $($s: _,)* .. } = self;
                $( $crate::savestate::State::load($f, r)?; )*
                Ok(())
            }
        }
    };
}

// Fieldless enum saved as one byte.
#[macro_export]
macro_rules! impl_state_enum {
    ($ty:ty { $($v:ident = $n:literal),* $(,)? }) => {
        impl $crate::savestate::State for $ty {
            fn save(&self, w: &mut Vec<u8>) {
                w.push(match self { $(Self::$v => $n,)* });
            }
            fn load(&mut self, r: &mut $crate::savestate::Reader) -> Result<(), $crate::savestate::StateError> {
                *self = match r.take(1)?[0] {
                    $($n => Self::$v,)*
                    _ => return Err($crate::savestate::StateError::Invalid(stringify!($ty))),
                };
                Ok(())
            }
        }
    };
}

fn cpu_options() -> impl bincode::Options {
    use bincode::Options;
    bincode::DefaultOptions::new().with_fixint_encoding().with_little_endian().with_limit(CPU_BLOB_MAX)
}

// The vendored m68k crate's own serde support: architectural state only, decode caches rebuild on demand.
impl State for m68k::CpuCore {
    fn save(&self, w: &mut Vec<u8>) {
        use bincode::Options;
        let start = w.len();
        0u64.save(w);
        let ok = cpu_options().serialize_into(&mut *w, self).is_ok();
        let n = if ok { (w.len() - start - 8) as u64 } else { u64::MAX };
        w[start..start + 8].copy_from_slice(&n.to_le_bytes());
    }
    fn load(&mut self, r: &mut Reader) -> Result<(), StateError> {
        use bincode::Options;
        let n = r.len()?;
        let cpu: m68k::CpuCore = cpu_options().deserialize(r.take(n)?).map_err(|e| StateError::Cpu(e.to_string()))?;
        *self = cpu;
        Ok(())
    }
}

// Word-wise FNV-1a; detects corruption, not tampering.
pub fn checksum(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut chunks = data.chunks_exact(8);
    for c in &mut chunks {
        h ^= u64::from_le_bytes(c.try_into().unwrap_or_default());
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for &b in chunks.remainder() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^ data.len() as u64
}

// What the state is bound to: BIOS, disc and DVC ROM are re-opened by path, never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Identity {
    pub bios: u64,
    pub disc: u64,
    pub dvc: u64,
}

pub struct Header {
    pub version: u32,
    pub payload_len: usize,
    pub frontend_len: usize,
    pub checksum: u64,
    pub id: Identity,
}

pub fn write_header(out: &mut [u8], payload_len: usize, frontend_len: usize, sum: u64, id: Identity) {
    let mut h = Vec::with_capacity(HEADER_LEN);
    h.extend_from_slice(&MAGIC);
    VERSION.save(&mut h);
    (HEADER_LEN as u32).save(&mut h);
    (payload_len as u64).save(&mut h);
    (frontend_len as u64).save(&mut h);
    sum.save(&mut h);
    id.bios.save(&mut h);
    id.disc.save(&mut h);
    id.dvc.save(&mut h);
    out[..HEADER_LEN].copy_from_slice(&h);
}

pub fn read_header(data: &[u8]) -> Result<Header, StateError> {
    if data.len() < HEADER_LEN {
        return Err(StateError::TooShort);
    }
    let mut r = Reader::new(&data[..HEADER_LEN]);
    if r.take(8)? != MAGIC {
        return Err(StateError::BadMagic);
    }
    let (mut version, mut hlen, mut payload, mut frontend, mut sum) = (0u32, 0u32, 0u64, 0u64, 0u64);
    let mut id = Identity::default();
    version.load(&mut r)?;
    if version != VERSION {
        return Err(StateError::Version(version));
    }
    hlen.load(&mut r)?;
    payload.load(&mut r)?;
    frontend.load(&mut r)?;
    sum.load(&mut r)?;
    id.bios.load(&mut r)?;
    id.disc.load(&mut r)?;
    id.dvc.load(&mut r)?;
    let total = (HEADER_LEN as u64).checked_add(payload).and_then(|t| t.checked_add(frontend));
    if hlen as usize != HEADER_LEN || total.is_none_or(|t| t > data.len() as u64) {
        return Err(StateError::TooShort);
    }
    Ok(Header { version, payload_len: payload as usize, frontend_len: frontend as usize, checksum: sum, id })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round<T: State + Default + PartialEq + fmt::Debug>(v: T) {
        let mut w = Vec::new();
        v.save(&mut w);
        let mut back = T::default();
        let mut r = Reader::new(&w);
        back.load(&mut r).unwrap();
        assert_eq!(r.remaining(), 0);
        assert_eq!(back, v);
    }

    #[test]
    fn primitives_and_containers_round_trip() {
        round(0x1234_5678u32);
        round(-3i16);
        round(1.5f64);
        round(true);
        round(Some(7u64));
        round(Option::<u8>::None);
        round(vec![1u8, 2, 3]);
        round(vec![(0.25f32, -0.5f32); 3]);
        round(VecDeque::from(vec![1u16, 2, 3]));
        round([[1u32; 3]; 2]);
    }

    #[test]
    fn bad_values_are_rejected() {
        let mut b = false;
        assert_eq!(b.load(&mut Reader::new(&[2])), Err(StateError::Invalid("bool")));
        let mut v: Vec<u8> = Vec::new();
        assert_eq!(v.load(&mut Reader::new(&u64::MAX.to_le_bytes())), Err(StateError::Invalid("collection length")));
        let mut x = 0u32;
        assert_eq!(x.load(&mut Reader::new(&[1, 2])), Err(StateError::Truncated));
    }

    #[test]
    fn machine_state_round_trips_and_rejects_mismatches() {
        let mut m = crate::machine::Machine::new();
        m.bus.sys_ram[5] = 0xAB;
        m.cpu.pc = 0x0040_04B8;
        let mut a = Vec::new();
        m.save_state(b"fe", &mut a);
        let mut n = crate::machine::Machine::new();
        let mut padded = a.clone();
        padded.resize(a.len() + 100, 0);
        assert_eq!(n.load_state(&padded), Ok(&b"fe"[..]));
        let mut b = Vec::new();
        n.save_state(b"fe", &mut b);
        assert_eq!(a, b);
        assert_eq!(n.cpu.pc, 0x0040_04B8);

        let mut bad = a.clone();
        bad[HEADER_LEN + 10] ^= 1;
        assert_eq!(n.load_state(&bad), Err(StateError::Checksum));
        bad = a.clone();
        bad[8] = 99;
        assert_eq!(n.load_state(&bad), Err(StateError::Version(99)));
        bad = a.clone();
        bad[0] = b'X';
        assert_eq!(n.load_state(&bad), Err(StateError::BadMagic));
        assert_eq!(n.load_state(&a[..a.len() - 1]), Err(StateError::TooShort));
        assert_eq!(n.load_state(&a[..10]), Err(StateError::TooShort));
        n.bus.dvc.load_rom_bytes(&[0x5A; 1024]);
        assert_eq!(n.load_state(&a), Err(StateError::DvcMismatch));
        let mut c = Vec::new();
        n.save_state(b"fe", &mut c);
        assert_eq!(crate::machine::Machine::new().load_state(&c), Err(StateError::DvcMismatch));
        n.bus.rom[0] = 1;
        assert_eq!(n.load_state(&a), Err(StateError::BiosMismatch));
        assert_eq!(n.bus.sys_ram[5], 0xAB);
    }
}
