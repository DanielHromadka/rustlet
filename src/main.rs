mod bootstrap;
mod install;
mod releases;
#[cfg(test)]
mod tests;
use serde_json::json;
use std::{
    ffi::{c_char, CStr, CString},
    path::Path,
};
extern "C" {
    fn rustlet_run();
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--verify-release") {
        let result = (|| {
            let path = args.get(2).ok_or("Chybí cesta k release.json.")?;
            let tool = releases::tool("desktop-tool-ring")?;
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let release = releases::decode(&tool, &bytes)?;
            if let Some(archive) = args.get(3) {
                let bytes = std::fs::read(archive).map_err(|e| e.to_string())?;
                if bytes.len() as u64 != release.archive_size {
                    return Err("Velikost archivu nesouhlasí.".into());
                }
                releases::verify(&bytes, &release.archive_signature, &tool.public_key)?;
            }
            println!(
                "Verified {} {} ({})",
                release.id, release.version, release.build
            );
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    unsafe {
        rustlet_run();
    }
}
unsafe fn input<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() {
        ""
    } else {
        CStr::from_ptr(ptr).to_str().unwrap_or("")
    }
}
fn output(value: serde_json::Value) -> *mut c_char {
    CString::new(value.to_string()).unwrap().into_raw()
}
#[no_mangle]
pub extern "C" fn mini_catalog() -> *mut c_char {
    output(serde_json::to_value(releases::catalog()).unwrap())
}
#[no_mangle]
/// # Safety
/// Input strings must be valid NUL-terminated UTF-8 for this call.
/// Free the returned JSON with mini_free.
pub unsafe extern "C" fn mini_check(
    id: *const c_char,
    os: *const c_char,
    installed: u64,
) -> *mut c_char {
    let result = releases::tool(input(id)).and_then(|tool| releases::latest(&tool, input(os)));
    output(match result {
        Ok(release) => {
            json!({"ok":true,"available":release.build>installed,"version":release.version,"build":release.build})
        }
        Err(error) => json!({"ok":false,"error":error}),
    })
}
#[no_mangle]
/// # Safety
/// Strings must remain valid for this call. stop is called on this worker thread
/// only after validation and must return -1 on refusal, 0 if stopped already,
/// or 1 after gracefully stopping the managed target. No forced kills occur.
pub unsafe extern "C" fn mini_install(
    id: *const c_char,
    target: *const c_char,
    os: *const c_char,
    stop: extern "C" fn(*const c_char, *const c_char) -> i32,
) -> *mut c_char {
    let mut relaunch = false;
    let result = (|| {
        let tool = releases::tool(input(id))?;
        if tool.integration == releases::Integration::External {
            return bootstrap::install(&tool, Path::new(input(target)), input(os));
        }
        let release = releases::latest(&tool, input(os))?;
        let (result, stopped) = install::install(&tool, &release, Path::new(input(target)), || {
            match stop(id, target) {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(
                    "Aplikaci se nepodařilo ukončit. Dokonči v ní práci a zkus aktualizaci znovu."
                        .into(),
                ),
            }
        });
        relaunch = stopped;
        result
    })();
    output(match result {
        Ok(()) => json!({"ok":true,"relaunch":relaunch}),
        Err(error) => json!({"ok":false,"error":error,"relaunch":relaunch}),
    })
}
#[no_mangle]
/// # Safety
/// value must be null or an unfreed allocation returned by mini_*.
pub unsafe extern "C" fn mini_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}
