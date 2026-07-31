//! kenro as a SQLite loadable extension.
//!
//! Exports three entry points so all common filenames load with a NULL
//! entry-point name (SQLite tries `sqlite3_extension_init` first, then a
//! name derived from the filename keeping only ASCII alphanumerics):
//!   - `sqlite3_extension_init` — any filename
//!   - `sqlite3_kenroext_init` — `libkenro_ext.so` / `kenro_ext.dll`
//!   - `sqlite3_kenro_init` — a renamed `libkenro.so` / `kenro.dll`
//!
//! Registration is per-connection (`Ok(false)` → `SQLITE_OK`); a
//! registration failure surfaces through `*pz_err_msg` with kenro's
//! `kenro:`-prefixed message.

use std::os::raw::{c_char, c_int};

use rusqlite::{Connection, Result, ffi};

fn init(conn: Connection) -> Result<bool> {
    kenro::register(&conn)?;
    Ok(false)
}

/// # Safety
/// Called by SQLite's extension loader with valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sqlite3_extension_init(
    db: *mut ffi::sqlite3,
    pz_err_msg: *mut *mut c_char,
    p_api: *mut ffi::sqlite3_api_routines,
) -> c_int {
    unsafe { Connection::extension_init2(db, pz_err_msg, p_api, init) }
}

/// # Safety
/// Called by SQLite's extension loader with valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sqlite3_kenroext_init(
    db: *mut ffi::sqlite3,
    pz_err_msg: *mut *mut c_char,
    p_api: *mut ffi::sqlite3_api_routines,
) -> c_int {
    unsafe { sqlite3_extension_init(db, pz_err_msg, p_api) }
}

/// # Safety
/// Called by SQLite's extension loader with valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sqlite3_kenro_init(
    db: *mut ffi::sqlite3,
    pz_err_msg: *mut *mut c_char,
    p_api: *mut ffi::sqlite3_api_routines,
) -> c_int {
    unsafe { sqlite3_extension_init(db, pz_err_msg, p_api) }
}
