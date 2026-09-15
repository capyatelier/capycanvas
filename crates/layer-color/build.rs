fn main() {
    println!("cargo:rerun-if-changed=src/photo/jpeg_codec.c");
    let jpeg = pkg_config::Config::new()
        .atleast_version("3.1")
        .probe("libjpeg")
        .expect("Install libjpeg-turbo development headers (3.1 or later)");
    cc::Build::new()
        .file("src/photo/jpeg_codec.c")
        .includes(jpeg.include_paths)
        .warnings(true)
        .compile("capy_jpeg");
}
