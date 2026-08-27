fn main() {
    println!("cargo:rerun-if-changed=src/gst_plugins.c");
    let mut build = cc::Build::new();
    build.file("src/gst_plugins.c");
    if std::path::Path::new("/opt/gstreamer-static/include/gstreamer-1.0").exists() {
        build
            .include("/opt/gstreamer-static/include/gstreamer-1.0")
            .include("/opt/glib-static/include/glib-2.0")
            .include("/opt/glib-static/lib/glib-2.0/include");
    } else if let Ok(output) = std::process::Command::new("pkg-config")
        .args(["--cflags-only-I", "gstreamer-1.0", "glib-2.0"])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            for flag in s.split_whitespace() {
                if let Some(path) = flag.strip_prefix("-I") {
                    build.include(path);
                }
            }
        }
    }
    build.compile("gst_plugins_init");
}
