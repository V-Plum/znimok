fn main() {
    #[cfg(target_os = "macos")]
    slint_build::compile("ui/p3.slint").expect("slint compile");
}
