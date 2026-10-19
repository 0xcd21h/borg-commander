fn main() {
    let licenses = two_face::acknowledgement::listing().to_md();
    for line in licenses.trim_end().lines() {
        println!("{}", line.trim_end());
    }
}
