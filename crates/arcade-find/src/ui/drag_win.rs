//! Dragging results out on Windows: a shell data object for the files
//! (CF_HDROP and the shell formats Explorer reads) and the shell's own drag
//! loop, which supplies the familiar drag image and cursors. Copy or link
//! only: the files never move.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Ole::{OleInitialize, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK};
use windows_sys::Win32::UI::Shell::Common::{ITEMIDLIST, SHITEMID};
use windows_sys::Win32::UI::Shell::{CIDLData_CreateFromIDArray, ILCreateFromPathW, ILFree, SHDoDragDrop};

/// `DRAGDROP_S_DROP`: the target accepted the drop.
const DRAGDROP_S_DROP: i32 = 0x0004_0100;

#[repr(C)]
struct UnknownVtbl {
    _query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    _add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// Runs the shell's modal drag loop for `paths` from `hwnd` (the left button
/// is down). True when a target took the drop.
pub fn drag(hwnd: isize, paths: &[PathBuf]) -> Result<bool, String> {
    // SAFETY: plain Win32/COM calls on the UI thread; every ID list we create
    // is freed, and the data object is released once the loop returns.
    unsafe {
        // Already initialized on the UI thread (S_FALSE) in practice.
        let _ = OleInitialize(std::ptr::null());
        let ids: Vec<*mut ITEMIDLIST> = paths
            .iter()
            .filter_map(|p| {
                let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
                let id = ILCreateFromPathW(wide.as_ptr());
                (!id.is_null()).then_some(id)
            })
            .collect();
        if ids.is_empty() {
            return Err("none of the files could be dragged".into());
        }
        // Absolute ID lists under the desktop (the empty ID list), so items
        // from different folders can travel together.
        let desktop = ITEMIDLIST { mkid: SHITEMID { cb: 0, abID: [0] } };
        let mut data: *mut c_void = std::ptr::null_mut();
        let hr = CIDLData_CreateFromIDArray(&desktop, ids.len() as u32, ids.as_ptr() as *const *const ITEMIDLIST, &mut data);
        for id in ids {
            ILFree(id);
        }
        if hr < 0 || data.is_null() {
            return Err(format!("CIDLData_CreateFromIDArray failed ({hr:#x})"));
        }
        let mut effect: DROPEFFECT = 0;
        let hr = SHDoDragDrop(hwnd as HWND, data, std::ptr::null_mut(), DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect);
        let vtbl = *(data as *const *const UnknownVtbl);
        ((*vtbl).release)(data);
        Ok(hr == DRAGDROP_S_DROP)
    }
}
