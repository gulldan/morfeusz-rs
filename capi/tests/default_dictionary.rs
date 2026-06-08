use std::ffi::{CStr, CString};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use morfeusz2::morfeusz_analyse;

#[test]
fn c_api_loads_default_dictionary_from_current_directory() {
    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).unwrap();
    fs::copy(
        fixture("test-dict-copyright-v1-a.dict"),
        temp_dir.join("sgjp-a.dict"),
    )
    .unwrap();
    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&temp_dir).unwrap();

    let input = CString::new("7").unwrap();
    let results = unsafe { morfeusz_analyse(input.as_ptr() as *mut _) };

    std::env::set_current_dir(old_dir).unwrap();
    fs::remove_dir_all(&temp_dir).unwrap();

    assert_eq!(unsafe { (*results.add(0)).p }, 0);
    assert_eq!(unsafe { (*results.add(0)).k }, 1);
    assert_eq!(unsafe { str_at((*results.add(0)).forma) }, "7");
    assert_eq!(unsafe { str_at((*results.add(0)).haslo) }, "7");
    assert_eq!(unsafe { str_at((*results.add(0)).interp) }, "dig");
    assert_eq!(unsafe { (*results.add(1)).p }, -1);
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../morfeusz-rs/tests/fixtures/binary")
        .join(name)
}

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("morfeusz-capi-default-dict-{nanos}"))
}

unsafe fn str_at(ptr: *mut std::os::raw::c_char) -> String {
    CStr::from_ptr(ptr).to_string_lossy().into_owned()
}
