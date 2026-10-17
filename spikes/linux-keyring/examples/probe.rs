//! Opens the real Secret Service key store and reports the typed outcome.
//! Use only on a disposable session bus, never a developer's own keyring.
fn main() {
    let dir = std::env::temp_dir().join("gist-keyring-probe");
    match gist_linux_keyring_spike::open_default(&dir) {
        Ok(_) => println!("OK: key resolved"),
        Err(error) => println!("ERR: {error:?}"),
    }
}
