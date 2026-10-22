fn main() {
    let raw = "1e-400";
    println!("raw parse: {:?}", raw.parse::<f64>());
    let sys: &[u8] = br#"{"answers":{"alignment":{"noul":"1e-400"}}}"#;
    let ali: &[u8] = br#"{"alignment":"1e-400"}"#;
    println!("systemone: {:?}", tributary_control::classifier::parse_systemone(sys));
    println!("alignment: {:?}", tributary_control::classifier::parse_alignment(ali));
    // same string via a different key the alignment parser also accepts
    let via_score: &[u8] = br#"{"score":"1e-400"}"#;
    println!("via score: {:?}", tributary_control::classifier::parse_alignment(via_score));
}
