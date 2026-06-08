use std::ffi::{CStr, CString};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use morfeusz2::morfeusz_analyse;

static REAL_SGJP_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn c_api_loads_real_sgjp_default_dictionary_from_current_directory() {
    let _guard = lock_real_sgjp_test();
    let dict_dir = Path::new("/tmp/morfeusz-sgjp-20260601");
    let analyzer = dict_dir.join("sgjp-a.dict");
    if !analyzer.exists() {
        eprintln!(
            "skipping real SGJP C API test because {} is missing",
            analyzer.display()
        );
        return;
    }

    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(dict_dir).unwrap();

    let input = CString::new("zażółć").unwrap();
    let results = unsafe { morfeusz_analyse(input.as_ptr() as *mut _) };

    std::env::set_current_dir(old_dir).unwrap();

    assert_eq!(unsafe { (*results.add(0)).p }, 0);
    assert_eq!(unsafe { (*results.add(0)).k }, 1);
    assert_eq!(unsafe { str_at((*results.add(0)).forma) }, "zażółć");
    assert_eq!(unsafe { str_at((*results.add(0)).haslo) }, "zażółcić");
    assert_eq!(
        unsafe { str_at((*results.add(0)).interp) },
        "impt:sg:sec:perf"
    );
    assert_eq!(unsafe { (*results.add(1)).p }, -1);
}

fn lock_real_sgjp_test() -> MutexGuard<'static, ()> {
    REAL_SGJP_LOCK.lock().unwrap()
}

unsafe fn str_at(ptr: *mut std::os::raw::c_char) -> String {
    CStr::from_ptr(ptr).to_string_lossy().into_owned()
}
