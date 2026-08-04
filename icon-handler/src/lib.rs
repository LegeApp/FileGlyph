#![cfg(windows)]
#![allow(linker_messages)]

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

type Hresult = i32;

const S_OK: Hresult = 0;
const S_FALSE: Hresult = 1;
const E_NOINTERFACE: Hresult = 0x8000_4002_u32 as i32;
const E_POINTER: Hresult = 0x8000_4003_u32 as i32;
const E_FAIL: Hresult = 0x8000_4005_u32 as i32;
const E_NOTIMPL: Hresult = 0x8000_4001_u32 as i32;
const CLASS_E_NOAGGREGATION: Hresult = 0x8004_0110_u32 as i32;
const CLASS_E_CLASSNOTAVAILABLE: Hresult = 0x8004_0111_u32 as i32;

const ASSOCF_NOTRUNCATE: u32 = 0x20;
const ASSOCF_INIT_IGNOREUNKNOWN: u32 = 0x400;
const ASSOCSTR_EXECUTABLE: u32 = 2;
const ASSOCSTR_PROGID: u32 = 20;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const IID_IUNKNOWN: Guid = guid(0x00000000, 0x0000, 0x0000, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_ICLASSFACTORY: Guid = guid(0x00000001, 0x0000, 0x0000, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_IPERSIST: Guid = guid(0x0000010C, 0x0000, 0x0000, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_IPERSISTFILE: Guid = guid(0x0000010B, 0x0000, 0x0000, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_IEXTRACTICONW: Guid = guid(0x000214FA, 0x0000, 0x0000, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const CLSID_FILEGLYPH: Guid = guid(
    0x5F7A3B34,
    0xEA94,
    0x4A97,
    [0xB0, 0x8F, 0x8D, 0x7D, 0xEA, 0x8C, 0xDF, 0x11],
);

const fn guid(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Guid {
    Guid {
        data1,
        data2,
        data3,
        data4,
    }
}

#[link(name = "Shlwapi")]
extern "system" {
    fn AssocQueryStringW(
        flags: u32,
        kind: u32,
        association: *const u16,
        extra: *const u16,
        output: *mut u16,
        output_chars: *mut u32,
    ) -> Hresult;
}

#[repr(C)]
struct Interface {
    vtable: *const c_void,
    owner: *mut Handler,
}

#[repr(C)]
struct Handler {
    persist: Interface,
    extract: Interface,
    refs: AtomicU32,
    path: Mutex<String>,
}

#[repr(C)]
struct PersistVtable {
    query_interface:
        unsafe extern "system" fn(*mut Interface, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(*mut Interface) -> u32,
    release: unsafe extern "system" fn(*mut Interface) -> u32,
    get_class_id: unsafe extern "system" fn(*mut Interface, *mut Guid) -> Hresult,
    is_dirty: unsafe extern "system" fn(*mut Interface) -> Hresult,
    load: unsafe extern "system" fn(*mut Interface, *const u16, u32) -> Hresult,
    save: unsafe extern "system" fn(*mut Interface, *const u16, i32) -> Hresult,
    save_completed: unsafe extern "system" fn(*mut Interface, *const u16) -> Hresult,
    get_cur_file: unsafe extern "system" fn(*mut Interface, *mut *mut u16) -> Hresult,
}

#[repr(C)]
struct ExtractVtable {
    query_interface:
        unsafe extern "system" fn(*mut Interface, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(*mut Interface) -> u32,
    release: unsafe extern "system" fn(*mut Interface) -> u32,
    get_icon_location: unsafe extern "system" fn(
        *mut Interface,
        u32,
        *mut u16,
        u32,
        *mut i32,
        *mut u32,
    ) -> Hresult,
    extract: unsafe extern "system" fn(
        *mut Interface,
        *const u16,
        u32,
        *mut *mut c_void,
        *mut *mut c_void,
        u32,
    ) -> Hresult,
}

static OBJECTS: AtomicU32 = AtomicU32::new(0);
static LOCKS: AtomicU32 = AtomicU32::new(0);

static PERSIST_VTABLE: PersistVtable = PersistVtable {
    query_interface: handler_query_interface,
    add_ref: handler_add_ref,
    release: handler_release,
    get_class_id,
    is_dirty,
    load,
    save,
    save_completed,
    get_cur_file,
};

static EXTRACT_VTABLE: ExtractVtable = ExtractVtable {
    query_interface: handler_query_interface,
    add_ref: handler_add_ref,
    release: handler_release,
    get_icon_location,
    extract,
};

unsafe fn handler_owner(this: *mut Interface) -> *mut Handler {
    (*this).owner
}

unsafe extern "system" fn handler_query_interface(
    this: *mut Interface,
    iid: *const Guid,
    output: *mut *mut c_void,
) -> Hresult {
    if iid.is_null() || output.is_null() {
        return E_POINTER;
    }
    *output = ptr::null_mut();
    let owner = handler_owner(this);
    if *iid == IID_IUNKNOWN || *iid == IID_IPERSIST || *iid == IID_IPERSISTFILE {
        *output = &mut (*owner).persist as *mut Interface as *mut c_void;
    } else if *iid == IID_IEXTRACTICONW {
        *output = &mut (*owner).extract as *mut Interface as *mut c_void;
    } else {
        return E_NOINTERFACE;
    }
    (*owner).refs.fetch_add(1, Ordering::Relaxed);
    S_OK
}

unsafe extern "system" fn handler_add_ref(this: *mut Interface) -> u32 {
    (*handler_owner(this)).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn handler_release(this: *mut Interface) -> u32 {
    let owner = handler_owner(this);
    let remaining = (*owner).refs.fetch_sub(1, Ordering::Release) - 1;
    if remaining == 0 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(owner));
        OBJECTS.fetch_sub(1, Ordering::Relaxed);
    }
    remaining
}

unsafe extern "system" fn get_class_id(_this: *mut Interface, output: *mut Guid) -> Hresult {
    if output.is_null() {
        return E_POINTER;
    }
    *output = CLSID_FILEGLYPH;
    S_OK
}

unsafe extern "system" fn is_dirty(_this: *mut Interface) -> Hresult {
    S_FALSE
}

unsafe extern "system" fn load(this: *mut Interface, filename: *const u16, _mode: u32) -> Hresult {
    let Some(value) = wide_string(filename) else {
        return E_POINTER;
    };
    match (*handler_owner(this)).path.lock() {
        Ok(mut path) => {
            *path = value;
            S_OK
        }
        Err(_) => E_FAIL,
    }
}

unsafe extern "system" fn save(
    _this: *mut Interface,
    _filename: *const u16,
    _remember: i32,
) -> Hresult {
    E_NOTIMPL
}

unsafe extern "system" fn save_completed(_this: *mut Interface, _filename: *const u16) -> Hresult {
    E_NOTIMPL
}

unsafe extern "system" fn get_cur_file(_this: *mut Interface, output: *mut *mut u16) -> Hresult {
    if !output.is_null() {
        *output = ptr::null_mut();
    }
    E_NOTIMPL
}

unsafe extern "system" fn get_icon_location(
    this: *mut Interface,
    _flags: u32,
    output: *mut u16,
    output_len: u32,
    index: *mut i32,
    result_flags: *mut u32,
) -> Hresult {
    if output.is_null() || index.is_null() || result_flags.is_null() {
        return E_POINTER;
    }
    let path = match (*handler_owner(this)).path.lock() {
        Ok(path) => path.clone(),
        Err(_) => return E_FAIL,
    };
    let Some(extension) = extension_of(&path) else {
        return E_FAIL;
    };
    let Some((location, location_index)) = resolve_icon(&extension) else {
        return E_FAIL;
    };
    let encoded: Vec<u16> = location.encode_utf16().collect();
    if encoded.len() + 1 > output_len as usize {
        return E_FAIL;
    }
    ptr::copy_nonoverlapping(encoded.as_ptr(), output, encoded.len());
    *output.add(encoded.len()) = 0;
    *index = location_index;
    *result_flags = 0;
    S_OK
}

unsafe extern "system" fn extract(
    _this: *mut Interface,
    _file: *const u16,
    _index: u32,
    large: *mut *mut c_void,
    small: *mut *mut c_void,
    _size: u32,
) -> Hresult {
    if !large.is_null() {
        *large = ptr::null_mut();
    }
    if !small.is_null() {
        *small = ptr::null_mut();
    }
    S_FALSE
}

fn extension_of(path: &str) -> Option<String> {
    let name = path.rsplit(['\\', '/']).next()?;
    let dot = name.rfind('.')?;
    (dot + 1 < name.len()).then(|| name[dot..].to_ascii_lowercase())
}

fn resolve_icon(extension: &str) -> Option<(String, i32)> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(map) = hkcu.open_subkey("Software\\FileGlyph\\IconMap") {
        if let Ok(path) = map.get_value::<String, _>(extension) {
            return Some((path, 0));
        }
    }

    let prog_id = assoc_query(extension, ASSOCSTR_PROGID);
    if let Some(prog_id) = prog_id.as_deref() {
        if let Ok(fallbacks) = hkcu.open_subkey("Software\\FileGlyph\\ProgIdFallbacks") {
            if let Ok(value) = fallbacks.get_value::<String, _>(prog_id) {
                return Some(parse_icon_location(&value));
            }
        }
    }
    assoc_query(extension, ASSOCSTR_EXECUTABLE).map(|path| (path, 0))
}

fn parse_icon_location(value: &str) -> (String, i32) {
    let trimmed = value.trim();
    if let Some((path, index)) = trimmed.rsplit_once(',') {
        if let Ok(index) = index.trim().parse() {
            return (path.trim().trim_matches('"').to_string(), index);
        }
    }
    (trimmed.trim_matches('"').to_string(), 0)
}

fn assoc_query(extension: &str, kind: u32) -> Option<String> {
    let association: Vec<u16> = extension.encode_utf16().chain(Some(0)).collect();
    let mut needed = 0;
    unsafe {
        let _ = AssocQueryStringW(
            ASSOCF_NOTRUNCATE | ASSOCF_INIT_IGNOREUNKNOWN,
            kind,
            association.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            &mut needed,
        );
    }
    if needed == 0 {
        return None;
    }
    let mut output = vec![0; needed as usize];
    let result = unsafe {
        AssocQueryStringW(
            ASSOCF_NOTRUNCATE | ASSOCF_INIT_IGNOREUNKNOWN,
            kind,
            association.as_ptr(),
            ptr::null(),
            output.as_mut_ptr(),
            &mut needed,
        )
    };
    if result < 0 {
        return None;
    }
    let end = output
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(output.len());
    Some(String::from_utf16_lossy(&output[..end]))
}

unsafe fn wide_string(value: *const u16) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let mut len = 0usize;
    while len < 32768 && *value.add(len) != 0 {
        len += 1;
    }
    (len < 32768).then(|| String::from_utf16_lossy(std::slice::from_raw_parts(value, len)))
}

fn create_handler() -> *mut Interface {
    let mut handler = Box::new(Handler {
        persist: Interface {
            vtable: &PERSIST_VTABLE as *const _ as *const c_void,
            owner: ptr::null_mut(),
        },
        extract: Interface {
            vtable: &EXTRACT_VTABLE as *const _ as *const c_void,
            owner: ptr::null_mut(),
        },
        refs: AtomicU32::new(1),
        path: Mutex::new(String::new()),
    });
    let owner = &mut *handler as *mut Handler;
    handler.persist.owner = owner;
    handler.extract.owner = owner;
    OBJECTS.fetch_add(1, Ordering::Relaxed);
    let interface = &mut handler.persist as *mut Interface;
    let _ = Box::into_raw(handler);
    interface
}

#[repr(C)]
struct Factory {
    vtable: *const FactoryVtable,
    refs: AtomicU32,
}

#[repr(C)]
struct FactoryVtable {
    query_interface:
        unsafe extern "system" fn(*mut Factory, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(*mut Factory) -> u32,
    release: unsafe extern "system" fn(*mut Factory) -> u32,
    create_instance: unsafe extern "system" fn(
        *mut Factory,
        *mut c_void,
        *const Guid,
        *mut *mut c_void,
    ) -> Hresult,
    lock_server: unsafe extern "system" fn(*mut Factory, i32) -> Hresult,
}

static FACTORY_VTABLE: FactoryVtable = FactoryVtable {
    query_interface: factory_query_interface,
    add_ref: factory_add_ref,
    release: factory_release,
    create_instance,
    lock_server,
};

unsafe extern "system" fn factory_query_interface(
    this: *mut Factory,
    iid: *const Guid,
    output: *mut *mut c_void,
) -> Hresult {
    if iid.is_null() || output.is_null() {
        return E_POINTER;
    }
    *output = ptr::null_mut();
    if *iid != IID_IUNKNOWN && *iid != IID_ICLASSFACTORY {
        return E_NOINTERFACE;
    }
    *output = this as *mut c_void;
    (*this).refs.fetch_add(1, Ordering::Relaxed);
    S_OK
}

unsafe extern "system" fn factory_add_ref(this: *mut Factory) -> u32 {
    (*this).refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn factory_release(this: *mut Factory) -> u32 {
    let remaining = (*this).refs.fetch_sub(1, Ordering::Release) - 1;
    if remaining == 0 {
        std::sync::atomic::fence(Ordering::Acquire);
        drop(Box::from_raw(this));
    }
    remaining
}

unsafe extern "system" fn create_instance(
    _this: *mut Factory,
    outer: *mut c_void,
    iid: *const Guid,
    output: *mut *mut c_void,
) -> Hresult {
    if !outer.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    if iid.is_null() || output.is_null() {
        return E_POINTER;
    }
    *output = ptr::null_mut();
    let interface = create_handler();
    let result = handler_query_interface(interface, iid, output);
    handler_release(interface);
    result
}

unsafe extern "system" fn lock_server(_this: *mut Factory, lock: i32) -> Hresult {
    if lock != 0 {
        LOCKS.fetch_add(1, Ordering::Relaxed);
    } else {
        LOCKS.fetch_sub(1, Ordering::Relaxed);
    }
    S_OK
}

#[no_mangle]
unsafe extern "system" fn DllGetClassObject(
    clsid: *const Guid,
    iid: *const Guid,
    output: *mut *mut c_void,
) -> Hresult {
    if clsid.is_null() || iid.is_null() || output.is_null() {
        return E_POINTER;
    }
    *output = ptr::null_mut();
    if *clsid != CLSID_FILEGLYPH {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory = Box::into_raw(Box::new(Factory {
        vtable: &FACTORY_VTABLE,
        refs: AtomicU32::new(1),
    }));
    let result = factory_query_interface(factory, iid, output);
    factory_release(factory);
    result
}

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> Hresult {
    if OBJECTS.load(Ordering::Acquire) == 0 && LOCKS.load(Ordering::Acquire) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_normalized_extension() {
        assert_eq!(extension_of(r"C:\Temp\THING.TxT").as_deref(), Some(".txt"));
        assert_eq!(extension_of(r"C:\Temp\no-extension"), None);
    }

    #[test]
    fn parses_resource_location() {
        assert_eq!(
            parse_icon_location(r#""C:\Windows\shell32.dll",-15"#),
            (r"C:\Windows\shell32.dll".to_string(), -15)
        );
    }
}
