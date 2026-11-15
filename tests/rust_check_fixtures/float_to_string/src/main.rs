fn main() {
    let value = std::hint::black_box(1.25_f64);
    let text = value.to_string();
    assert!(!text.is_empty());
}
