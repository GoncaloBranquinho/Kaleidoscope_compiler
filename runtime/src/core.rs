use std::io::Write;

#[unsafe(no_mangle)]
pub extern "C" fn putchard(x: f64) {
    print!("{}", x as u8 as char);
    std::io::stdout().flush().unwrap();
}

#[unsafe(no_mangle)]
pub extern "C" fn printd(x: f64) {
    println!("{}", x);
}
