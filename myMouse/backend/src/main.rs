mod discovery;
mod input;
mod layout;
mod pairing;
mod test_mode;
mod trust;
mod transport;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(config) = test_mode::parse_args(&args) {
        if let Err(error) = test_mode::run(config).await {
            eprintln!("mymouse test mode failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    println!("mymouse-backend");
}

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mymouse-backend");
    }
}
