use std::os::raw::c_double;

#[unsafe(no_mangle)]
pub extern "C" fn putchard(x: c_double) -> c_double {
    print!("{}", x as u8 as char);
    0.0
}
