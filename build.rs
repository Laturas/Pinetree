// build.rs

#[cfg(target_os = "windows")]
fn main() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("./resources/Pinetree Logo.ico");
    res.compile().expect("Failed to compile Windows resource file");
}

#[cfg(not(target_os = "windows"))]
fn main() {}