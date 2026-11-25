//! A minimal WebAssembly entry point for the AnvilCAD kernel (see docs/WEB.md).
//!
//! The host (a browser page, or Node in scripts/wasm-check.sh) copies a file's text into
//! memory from `alloc`, calls `rebuild_part` or `import_step`, then reads the result text
//! through `out_ptr` and `out_len`. No file system, threads or clock are used: files come
//! in and go out as bytes, the way a browser upload and download would carry them.

use std::cell::RefCell;

thread_local! {
    static OUT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Rebuilds a saved part (`.anvprt` JSON) and reports its volume, area and STEP size.
pub fn rebuild_part_text(json: &str) -> Result<String, String> {
    let mut doc = anvil_model::Document::from_json(json).map_err(|e| e.to_string())?;
    let state = doc.rebuild();
    let props = state.body.mass_properties();
    let step = anvil_model::step::exact_step(&state, "part").unwrap_or_default();
    Ok(format!(
        "features={} errors={} volume={:.6} area={:.6} step_bytes={}",
        doc.features.len(),
        state.errors.len(),
        props.volume,
        props.surface_area,
        step.len()
    ))
}

/// Imports a STEP file and reports what it found.
pub fn import_step_text(text: &str) -> Result<String, String> {
    let imp = anvil_step::import(text).map_err(|e| e.to_string())?;
    let exact = imp.bodies.iter().filter(|b| b.exact.is_some()).count();
    let tris: usize =
        imp.bodies.iter().map(|b| b.mesh.faces.iter().map(|f| f.triangles.len()).sum::<usize>()).sum();
    Ok(format!("bodies={} exact={} triangles={}", imp.bodies.len(), exact, tris))
}

#[cfg(target_arch = "wasm32")]
mod ffi {
    #![allow(unsafe_code)]

    fn input(ptr: *mut u8, len: usize) -> String {
        // SAFETY: the host wrote `len` bytes at `ptr`, a buffer it got from `alloc(len)`.
        let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn finish(r: Result<String, String>) -> i32 {
        let ok = r.is_ok();
        super::OUT.with(|o| *o.borrow_mut() = r.unwrap_or_else(|e| e));
        if ok {
            0
        } else {
            1
        }
    }

    #[no_mangle]
    pub extern "C" fn alloc(len: usize) -> *mut u8 {
        let mut v = Vec::<u8>::with_capacity(len);
        let p = v.as_mut_ptr();
        std::mem::forget(v);
        p
    }

    #[no_mangle]
    pub extern "C" fn rebuild_part(ptr: *mut u8, len: usize) -> i32 {
        finish(super::rebuild_part_text(&input(ptr, len)))
    }

    #[no_mangle]
    pub extern "C" fn import_step(ptr: *mut u8, len: usize) -> i32 {
        finish(super::import_step_text(&input(ptr, len)))
    }

    #[no_mangle]
    pub extern "C" fn out_ptr() -> *const u8 {
        super::OUT.with(|o| o.borrow().as_ptr())
    }

    #[no_mangle]
    pub extern "C" fn out_len() -> usize {
        super::OUT.with(|o| o.borrow().len())
    }
}
