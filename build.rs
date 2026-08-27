fn main() {
    println!("cargo:rerun-if-changed=src/gst_plugins.c");
    cc::Build::new()
        .file("src/gst_plugins.c")
        .include("/opt/gstreamer-static/include/gstreamer-1.0")
        .include("/opt/glib-static/include/glib-2.0")
        .include("/opt/glib-static/lib/glib-2.0/include")
        .compile("gst_plugins_init");
}
