fn main() {
    // Emits the pyo3 cfgs for the target interpreter (incl. `Py_GIL_DISABLED`
    // on free-threaded builds) so the crate can specialize on `#[cfg(...)]`, and
    // registers them with `rustc-check-cfg` to avoid unknown-cfg warnings.
    pyo3_build_config::use_pyo3_cfgs();
    pyo3_build_config::add_extension_module_link_args();
}
