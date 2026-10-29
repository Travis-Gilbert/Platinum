//! Mozilla NSS owns Firefox's SDR cryptography. Only encrypted database
//! snapshots enter the temporary directory; cleartext stays in zeroing buffers.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use libloading::Library;
use std::{
    ffi::{CString, c_int, c_uint, c_void},
    path::Path,
    ptr,
    sync::{Mutex, MutexGuard},
};
use tempfile::TempDir;
use zeroize::Zeroizing;

static NSS: Mutex<()> = Mutex::new(());

// ABI from the installed Mozilla NSS seccomon.h / pk11sdr.h headers.
#[repr(C)]
struct Item {
    kind: c_uint,
    data: *mut u8,
    len: c_uint,
}
impl Default for Item {
    fn default() -> Self {
        Self {
            kind: 0,
            data: ptr::null_mut(),
            len: 0,
        }
    }
}
type Decrypt = unsafe extern "C" fn(*mut Item, *mut Item, *mut c_void) -> c_int;
type Free = unsafe extern "C" fn(*mut Item, c_int);
type Shutdown = unsafe extern "C" fn() -> c_int;

pub(super) struct Profile {
    decrypt: Decrypt,
    free: Free,
    shutdown: Shutdown,
    // Keep the library, encrypted snapshot and lock alive until after shutdown.
    _library: Library,
    _directory: TempDir,
    _guard: MutexGuard<'static, ()>,
}
impl Profile {
    pub fn open(directory: &Path) -> Result<Self> {
        let guard = NSS
            .lock()
            .map_err(|_| anyhow::anyhow!("Firefox password service is unavailable"))?;
        let temporary = tempfile::tempdir()?;
        for name in ["key4.db", "cert9.db"] {
            let source = directory.join(name);
            if source.is_file() {
                super::snapshot(&source)?.backup("main", temporary.path().join(name), None)?;
            }
        }
        ensure!(
            temporary.path().join("key4.db").is_file(),
            "Firefox encryption database was not found"
        );
        let path = [
            "/opt/homebrew/opt/nss/lib/libnss3.dylib",
            "/usr/local/opt/nss/lib/libnss3.dylib",
            "/Applications/Firefox.app/Contents/MacOS/libnss3.dylib",
        ]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .context("Mozilla NSS is unavailable; use Firefox password CSV export")?;
        // SAFETY: fixed installed-library paths and exact NSS C signatures. Every
        // symbol stays within the owning Library's lifetime; access is serialized.
        unsafe {
            let library = Library::new(path).map_err(|_| {
                anyhow::anyhow!("Could not load Mozilla NSS; use Firefox password CSV export")
            })?;
            let init = *library
                .get::<unsafe extern "C" fn(*const std::ffi::c_char) -> c_int>(b"NSS_Init\0")?;
            let initialized =
                *library.get::<unsafe extern "C" fn() -> c_int>(b"NSS_IsInitialized\0")?;
            let shutdown = *library.get::<Shutdown>(b"NSS_Shutdown\0")?;
            let decrypt = *library.get::<Decrypt>(b"PK11SDR_Decrypt\0")?;
            let free = *library.get::<Free>(b"SECITEM_ZfreeItem\0")?;
            let get_slot = *library
                .get::<unsafe extern "C" fn() -> *mut c_void>(b"PK11_GetInternalKeySlot\0")?;
            let free_slot =
                *library.get::<unsafe extern "C" fn(*mut c_void)>(b"PK11_FreeSlot\0")?;
            let check_password = *library
                .get::<unsafe extern "C" fn(*mut c_void, *const std::ffi::c_char) -> c_int>(
                    b"PK11_CheckUserPassword\0",
                )?;
            ensure!(
                initialized() == 0,
                "Mozilla NSS is already in use; use Firefox password CSV export"
            );
            let config = CString::new(format!("sql:{}", temporary.path().display()))?;
            ensure!(
                init(config.as_ptr()) == 0,
                "Could not open Firefox encryption database"
            );
            let profile = Self {
                decrypt,
                free,
                shutdown,
                _library: library,
                _directory: temporary,
                _guard: guard,
            };
            let slot = get_slot();
            ensure!(!slot.is_null(), "Firefox encryption key is unavailable");
            let status = check_password(slot, c"".as_ptr());
            free_slot(slot);
            ensure!(
                status == 0,
                "Firefox Primary Password is required; export passwords from Firefox and use CSV import"
            );
            Ok(profile)
        }
    }

    pub fn decrypt(&self, encoded: &str) -> Result<Zeroizing<String>> {
        let mut encrypted = STANDARD
            .decode(encoded)
            .context("Invalid Firefox encrypted record")?;
        let mut input = Item {
            kind: 0,
            data: encrypted.as_mut_ptr(),
            len: encrypted.len().try_into()?,
        };
        let mut output = Item::default();
        // SAFETY: input owns its bounded buffer. NSS allocates output; copy only
        // while valid and zero/free it on success and failure before returning.
        unsafe {
            let status = (self.decrypt)(&mut input, &mut output, ptr::null_mut());
            let result = if status == 0 && !output.data.is_null() {
                std::str::from_utf8(std::slice::from_raw_parts(output.data, output.len as usize))
                    .map(|s| Zeroizing::new(s.to_owned()))
                    .map_err(|_| anyhow::anyhow!("Invalid Firefox credential encoding"))
            } else {
                Err(anyhow::anyhow!("Could not decrypt Firefox credential"))
            };
            (self.free)(&mut output, 0);
            result
        }
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        // SAFETY: this object owns the only initialized context in this module.
        unsafe {
            (self.shutdown)();
        }
    }
}

#[cfg(test)]
pub(super) fn fixture(directory: &Path) -> Result<(String, String)> {
    let _guard = NSS.lock().unwrap();
    ensure!(
        std::process::Command::new("/opt/homebrew/opt/nss/bin/certutil")
            .args(["-N", "--empty-password", "-d"])
            .arg(format!("sql:{}", directory.display()))
            .status()?
            .success(),
        "Could not initialize synthetic NSS database"
    );
    // SAFETY: same installed NSS ABI as the production reader. This writer is
    // test-only and only receives a newly created temporary profile.
    unsafe {
        let library = Library::new("/opt/homebrew/opt/nss/lib/libnss3.dylib")?;
        let init = library.get::<unsafe extern "C" fn(*const std::ffi::c_char) -> c_int>(
            b"NSS_InitReadWrite\0",
        )?;
        let shutdown = *library.get::<Shutdown>(b"NSS_Shutdown\0")?;
        let free = *library.get::<Free>(b"SECITEM_ZfreeItem\0")?;
        let encrypt =
            library
                .get::<unsafe extern "C" fn(*mut Item, *mut Item, *mut Item, *mut c_void) -> c_int>(
                    b"PK11SDR_Encrypt\0",
                )?;
        let config = CString::new(format!("sql:{}", directory.display()))?;
        ensure!(
            init(config.as_ptr()) == 0,
            "Could not initialize NSS fixture writer"
        );
        struct Cleanup(Shutdown);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                unsafe {
                    (self.0)();
                }
            }
        }
        let _cleanup = Cleanup(shutdown);
        let encode = |text: &str| -> Result<String> {
            let mut key = Item::default();
            let mut input = Item {
                kind: 0,
                data: text.as_ptr().cast_mut(),
                len: text.len() as u32,
            };
            let mut output = Item::default();
            let status = encrypt(&mut key, &mut input, &mut output, ptr::null_mut());
            let result = if status == 0 && !output.data.is_null() {
                Ok(STANDARD.encode(std::slice::from_raw_parts(output.data, output.len as usize)))
            } else {
                Err(anyhow::anyhow!("Could not encrypt synthetic NSS record"))
            };
            free(&mut output, 0);
            free(&mut key, 0);
            result
        };
        Ok((encode("fixture-user")?, encode("fixture-secret")?))
    }
}
