//! Minimal AppleSMC client over IOKit.
//!
//! Reading works as a normal user. Writing (fans, charging) requires root and
//! is only used by the privileged helper. Struct layout mirrors the
//! `SMCKeyData_t` used by Apple's own `smc` tool (80 bytes).

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::sync::Mutex;

use thiserror::Error;

type KernReturn = i32;
type MachPort = u32;
type IoObject = MachPort;
type IoConnect = MachPort;

const KERN_SUCCESS: KernReturn = 0;
const KERNEL_INDEX_SMC: u32 = 2;
const SMC_CMD_READ_BYTES: u8 = 5;
const SMC_CMD_WRITE_BYTES: u8 = 6;
const SMC_CMD_READ_INDEX: u8 = 8;
const SMC_CMD_READ_KEYINFO: u8 = 9;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    static kIOMainPortDefault: MachPort;
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(main_port: MachPort, matching: *mut c_void) -> IoObject;
    fn IOServiceOpen(service: IoObject, owning_task: MachPort, kind: u32, connect: *mut IoConnect) -> KernReturn;
    fn IOServiceClose(connect: IoConnect) -> KernReturn;
    fn IOObjectRelease(object: IoObject) -> KernReturn;
    fn IOConnectCallStructMethod(
        connection: MachPort,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> KernReturn;
}

extern "C" {
    static mach_task_self_: MachPort;
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcVersion {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcPLimitData {
    version: u16,
    length: u16,
    cpu_p_limit: u32,
    gpu_p_limit: u32,
    mem_p_limit: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcKeyInfoData {
    data_size: u32,
    data_type: u32,
    data_attributes: u8,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmcKeyData {
    key: u32,
    vers: SmcVersion,
    p_limit_data: SmcPLimitData,
    key_info: SmcKeyInfoData,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}

const _: () = assert!(std::mem::size_of::<SmcKeyData>() == 80);

#[derive(Debug, Error)]
pub enum SmcError {
    #[error("AppleSMC service not found")]
    NotFound,
    #[error("IOServiceOpen failed: {0:#x}")]
    Open(i32),
    #[error("SMC call failed for {key}: kern={kern:#x} result={result}")]
    Call { key: String, kern: i32, result: u8 },
    #[error("invalid SMC key {0:?} (must be 4 ASCII chars)")]
    BadKey(String),
    #[error("value for {key} has {got} bytes, SMC expects {expected}")]
    Size { key: String, got: usize, expected: usize },
    #[error("unsupported SMC type {0:?}")]
    Type(String),
}

pub type Result<T> = std::result::Result<T, SmcError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInfo {
    pub size: u32,
    pub data_type: [u8; 4],
}

impl KeyInfo {
    pub fn type_str(&self) -> String {
        String::from_utf8_lossy(&self.data_type).to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SmcValue {
    pub key: String,
    pub info: KeyInfo,
    pub bytes: Vec<u8>,
}

impl SmcValue {
    /// Decodes numeric SMC types into f64. Returns None for opaque types.
    pub fn as_f64(&self) -> Option<f64> {
        decode(&self.info.data_type, &self.bytes)
    }
}

pub fn decode(data_type: &[u8; 4], b: &[u8]) -> Option<f64> {
    let t = std::str::from_utf8(data_type).ok()?;
    let be16 = |b: &[u8]| u16::from_be_bytes([b[0], b[1]]);
    Some(match t {
        // Apple Silicon reports floats and plain integers in little-endian
        // (verified on Apple Silicon: a ui32 like 28 0d 00 00 decodes to 3368, matching ioreg).
        "flt " if b.len() >= 4 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
        "ui8 " | "ui8\0" if !b.is_empty() => b[0] as f64,
        "ui16" if b.len() >= 2 => native16(b) as f64,
        "ui32" if b.len() >= 4 => native32(b) as f64,
        "ui64" if b.len() >= 8 => native64(b) as f64,
        "si8 " if !b.is_empty() => b[0] as i8 as f64,
        "si16" if b.len() >= 2 => native16(b) as i16 as f64,
        "si32" if b.len() >= 4 => native32(b) as i32 as f64,
        "flag" if !b.is_empty() => b[0] as f64,
        "fpe2" if b.len() >= 2 => be16(b) as f64 / 4.0,
        "sp78" if b.len() >= 2 => be16(b) as i16 as f64 / 256.0,
        "sp87" if b.len() >= 2 => be16(b) as i16 as f64 / 128.0,
        "sp96" if b.len() >= 2 => be16(b) as i16 as f64 / 64.0,
        "sp5a" if b.len() >= 2 => be16(b) as i16 as f64 / 1024.0,
        "fp88" if b.len() >= 2 => be16(b) as f64 / 256.0,
        _ => return None,
    })
}

const LITTLE_ENDIAN_INTS: bool = cfg!(target_arch = "aarch64");

fn native16(b: &[u8]) -> u16 {
    let a = [b[0], b[1]];
    if LITTLE_ENDIAN_INTS { u16::from_le_bytes(a) } else { u16::from_be_bytes(a) }
}

fn native32(b: &[u8]) -> u32 {
    let a = [b[0], b[1], b[2], b[3]];
    if LITTLE_ENDIAN_INTS { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) }
}

fn native64(b: &[u8]) -> u64 {
    let a: [u8; 8] = b[..8].try_into().unwrap();
    if LITTLE_ENDIAN_INTS { u64::from_le_bytes(a) } else { u64::from_be_bytes(a) }
}

/// Encodes a number into the byte layout for the given SMC type.
pub fn encode(data_type: &[u8; 4], size: usize, value: f64) -> Result<Vec<u8>> {
    let t = String::from_utf8_lossy(data_type).to_string();
    let mut out = match t.as_str() {
        "flt " => (value as f32).to_le_bytes().to_vec(),
        "ui8 " | "flag" => vec![value as u8],
        "ui16" if LITTLE_ENDIAN_INTS => (value as u16).to_le_bytes().to_vec(),
        "ui16" => (value as u16).to_be_bytes().to_vec(),
        "ui32" if LITTLE_ENDIAN_INTS => (value as u32).to_le_bytes().to_vec(),
        "ui32" => (value as u32).to_be_bytes().to_vec(),
        "fpe2" => ((value * 4.0) as u16).to_be_bytes().to_vec(),
        "hex_" => {
            // Raw bytes: interpret value as an unsigned integer, big-endian, sized to the key.
            let v = value as u64;
            let full = v.to_be_bytes();
            full[8 - size.min(8)..].to_vec()
        }
        _ => return Err(SmcError::Type(t)),
    };
    out.resize(size, 0);
    Ok(out)
}

fn key_to_u32(key: &str) -> Result<u32> {
    let b = key.as_bytes();
    if b.len() != 4 || !key.is_ascii() {
        return Err(SmcError::BadKey(key.to_string()));
    }
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn u32_to_key(v: u32) -> String {
    String::from_utf8_lossy(&v.to_be_bytes()).to_string()
}

pub struct Smc {
    conn: IoConnect,
    cache: Mutex<HashMap<u32, KeyInfo>>,
}

// The connection handle is a Mach port: safe to share; calls are serialized by IOKit.
unsafe impl Send for Smc {}
unsafe impl Sync for Smc {}

impl Smc {
    pub fn open() -> Result<Self> {
        let name = CString::new("AppleSMC").unwrap();
        unsafe {
            let matching = IOServiceMatching(name.as_ptr());
            let service = IOServiceGetMatchingService(kIOMainPortDefault, matching);
            if service == 0 {
                return Err(SmcError::NotFound);
            }
            let mut conn: IoConnect = 0;
            let kr = IOServiceOpen(service, mach_task_self_, 0, &mut conn);
            IOObjectRelease(service);
            if kr != KERN_SUCCESS {
                return Err(SmcError::Open(kr));
            }
            Ok(Self { conn, cache: Mutex::new(HashMap::new()) })
        }
    }

    fn call(&self, input: &SmcKeyData) -> Result<SmcKeyData> {
        let mut output = SmcKeyData::default();
        let mut out_size = std::mem::size_of::<SmcKeyData>();
        let kr = unsafe {
            IOConnectCallStructMethod(
                self.conn,
                KERNEL_INDEX_SMC,
                input as *const _ as *const c_void,
                std::mem::size_of::<SmcKeyData>(),
                &mut output as *mut _ as *mut c_void,
                &mut out_size,
            )
        };
        if kr != KERN_SUCCESS || output.result != 0 {
            return Err(SmcError::Call { key: u32_to_key(input.key), kern: kr, result: output.result });
        }
        Ok(output)
    }

    pub fn key_info(&self, key: &str) -> Result<KeyInfo> {
        let k = key_to_u32(key)?;
        if let Some(info) = self.cache.lock().unwrap().get(&k) {
            return Ok(*info);
        }
        let input = SmcKeyData { key: k, data8: SMC_CMD_READ_KEYINFO, ..Default::default() };
        let out = self.call(&input)?;
        let info = KeyInfo { size: out.key_info.data_size, data_type: out.key_info.data_type.to_be_bytes() };
        self.cache.lock().unwrap().insert(k, info);
        Ok(info)
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.key_info(key).is_ok()
    }

    pub fn read(&self, key: &str) -> Result<SmcValue> {
        let info = self.key_info(key)?;
        let mut input = SmcKeyData { key: key_to_u32(key)?, data8: SMC_CMD_READ_BYTES, ..Default::default() };
        input.key_info.data_size = info.size;
        let out = self.call(&input)?;
        let len = (info.size as usize).min(32);
        Ok(SmcValue { key: key.to_string(), info, bytes: out.bytes[..len].to_vec() })
    }

    pub fn read_f64(&self, key: &str) -> Option<f64> {
        self.read(key).ok()?.as_f64()
    }

    /// Writes raw bytes. Requires root.
    pub fn write_bytes(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let info = self.key_info(key)?;
        if bytes.len() != info.size as usize {
            return Err(SmcError::Size { key: key.into(), got: bytes.len(), expected: info.size as usize });
        }
        let mut input = SmcKeyData { key: key_to_u32(key)?, data8: SMC_CMD_WRITE_BYTES, ..Default::default() };
        input.key_info.data_size = info.size;
        input.bytes[..bytes.len()].copy_from_slice(bytes);
        self.call(&input).map(|_| ())
    }

    /// Encodes `value` using the key's own type and writes it. Requires root.
    pub fn write_number(&self, key: &str, value: f64) -> Result<()> {
        let info = self.key_info(key)?;
        let bytes = encode(&info.data_type, info.size as usize, value)?;
        self.write_bytes(key, &bytes)
    }

    pub fn key_count(&self) -> u32 {
        // #KEY is big-endian even on Apple Silicon.
        self.read("#KEY")
            .ok()
            .and_then(|v| v.bytes.get(..4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])))
            .unwrap_or(0)
    }

    pub fn key_at(&self, index: u32) -> Result<String> {
        let input = SmcKeyData { data8: SMC_CMD_READ_INDEX, data32: index, ..Default::default() };
        let out = self.call(&input)?;
        Ok(u32_to_key(out.key))
    }

    /// Lists every key the SMC exposes (a few thousand on Apple Silicon).
    pub fn all_keys(&self) -> Vec<String> {
        (0..self.key_count()).filter_map(|i| self.key_at(i).ok()).collect()
    }
}

impl Drop for Smc {
    fn drop(&mut self) {
        unsafe {
            IOServiceClose(self.conn);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_common_types() {
        assert_eq!(decode(b"flt ", &42.5f32.to_le_bytes()), Some(42.5));
        assert_eq!(decode(b"ui8 ", &[7]), Some(7.0));
        assert_eq!(decode(b"ui32", &[0x28, 0x0d, 0, 0]), Some(3368.0));
        assert_eq!(decode(b"sp78", &[0x28, 0x80]), Some(40.5));
        assert_eq!(decode(b"fpe2", &[0x1f, 0x40]), Some(2000.0));
    }

    #[test]
    fn encode_roundtrip() {
        let b = encode(b"flt ", 4, 1800.0).unwrap();
        assert_eq!(decode(b"flt ", &b), Some(1800.0));
        let b = encode(b"ui32", 4, 1.0).unwrap();
        assert_eq!(b, vec![1, 0, 0, 0]);
    }

    #[test]
    fn rejects_bad_keys() {
        assert!(key_to_u32("TOOLONG").is_err());
        assert_eq!(u32_to_key(key_to_u32("F0Ac").unwrap()), "F0Ac");
    }
}
