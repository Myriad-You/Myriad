//! The shared core: Myriad's pure persona rules (`myriad-merope`,
//! `myriad-agent-rules`) behind one C entry point, so that another platform
//! (the native app) runs the same code as the site instead of a copy of it.
//!
//! The ABI is two symbols. [`myriad_core_call`] takes a call name and a JSON
//! object and returns JSON in a buffer the caller owns; [`myriad_core_buf_free`]
//! gives the buffer back. Every call is a pure function of its input: the
//! time, her zone and any state come in the payload, nothing is read from the
//! process. `meta.version` names the ABI version, the upstream commit the
//! library was built from, and every call it has.

use std::{ffi::CStr, os::raw::c_char, panic};

use serde_json::{Value, json};

mod calls;

/// Bumped when a call's input or output changes incompatibly, or a call is
/// removed. Adding a call or an optional field does not bump it.
pub const ABI_VERSION: u32 = 1;

/// The upstream commit this library was built from (see `build.rs`).
pub const UPSTREAM_COMMIT: &str = env!("MYRIAD_CORE_UPSTREAM_COMMIT");

pub const STATUS_OK: i32 = 0;
pub const STATUS_UNKNOWN_CALL: i32 = 1;
pub const STATUS_BAD_INPUT: i32 = 2;
pub const STATUS_PANIC: i32 = 3;

/// A buffer of JSON owned by the caller until it is passed back to
/// [`myriad_core_buf_free`].
#[repr(C)]
pub struct MyriadCoreBuf {
    pub ptr: *mut u8,
    pub len: usize,
    pub status: i32,
}

/// Why a call gave no answer.
#[derive(Debug, PartialEq)]
pub enum Failure {
    UnknownCall(String),
    BadInput(String),
}

impl Failure {
    fn status(&self) -> i32 {
        match self {
            Self::UnknownCall(_) => STATUS_UNKNOWN_CALL,
            Self::BadInput(_) => STATUS_BAD_INPUT,
        }
    }

    fn payload(&self) -> Value {
        let (kind, message) = match self {
            Self::UnknownCall(name) => ("unknown_call", format!("no call named {name:?}")),
            Self::BadInput(message) => ("bad_input", message.clone()),
        };
        json!({ "error": { "kind": kind, "message": message } })
    }
}

/// Every call name, sorted.
pub fn call_names() -> Vec<&'static str> {
    calls::names()
}

/// [`myriad_core_call`] without the C ABI: the status and the JSON bytes.
pub fn call(name: &str, input: &[u8]) -> (i32, Vec<u8>) {
    call_with(name, input, |input| calls::dispatch(name, input))
}

/// Runs `run` as the call `name`: a panic inside it is caught and reported,
/// never unwound into the caller's frames.
fn call_with<F>(name: &str, input: &[u8], run: F) -> (i32, Vec<u8>)
where
    F: FnOnce(&[u8]) -> Result<Value, Failure> + panic::UnwindSafe,
{
    let answered = panic::catch_unwind(|| run(input));
    let (status, value) = match answered {
        Ok(Ok(value)) => (STATUS_OK, value),
        Ok(Err(failure)) => (failure.status(), failure.payload()),
        Err(_) => (
            STATUS_PANIC,
            json!({ "error": { "kind": "panic", "message": format!("{name} panicked") } }),
        ),
    };
    // A `Value` always serializes: map keys are strings.
    (status, serde_json::to_vec(&value).unwrap_or_default())
}

fn owned(status: i32, bytes: Vec<u8>) -> MyriadCoreBuf {
    let bytes = Box::into_raw(bytes.into_boxed_slice());
    MyriadCoreBuf {
        ptr: bytes.cast::<u8>(),
        len: bytes.len(),
        status,
    }
}

fn refused(status: i32, failure: &Failure) -> MyriadCoreBuf {
    owned(
        status,
        serde_json::to_vec(&failure.payload()).unwrap_or_default(),
    )
}

/// Runs the call `name` on the JSON object at `input`.
///
/// # Safety
/// `name` is null or points at a NUL-terminated string. `input` points at
/// `len` readable bytes, or is null when `len` is 0 (read as `{}`). The
/// returned buffer is the caller's: pass it to [`myriad_core_buf_free`]
/// exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn myriad_core_call(
    name: *const c_char,
    input: *const u8,
    len: usize,
) -> MyriadCoreBuf {
    if name.is_null() {
        let failure = Failure::BadInput("call name is null".into());
        return refused(failure.status(), &failure);
    }
    // SAFETY: non-null and NUL-terminated, per the contract above.
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        let failure = Failure::BadInput("call name is not UTF-8".into());
        return refused(failure.status(), &failure);
    };
    let input: &[u8] = if len == 0 {
        b"{}"
    } else if input.is_null() {
        let failure = Failure::BadInput("input is null but len is not 0".into());
        return refused(failure.status(), &failure);
    } else {
        // SAFETY: `len` readable bytes at `input`, per the contract above.
        unsafe { std::slice::from_raw_parts(input, len) }
    };
    let (status, bytes) = call(name, input);
    owned(status, bytes)
}

/// Gives back a buffer from [`myriad_core_call`].
///
/// # Safety
/// `buf` came from [`myriad_core_call`] and has not been freed. A buffer
/// with a null `ptr` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn myriad_core_buf_free(buf: MyriadCoreBuf) {
    if buf.ptr.is_null() {
        return;
    }
    // SAFETY: `ptr`/`len` are the boxed slice `owned` leaked, freed once.
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(buf.ptr, buf.len)) });
}

#[cfg(test)]
mod tests;
