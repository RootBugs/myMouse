mod discovery;
mod pairing;
mod trust;

fn main() {
    println!("mymouse-backend");
}

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mymouse-backend");
    }
}
