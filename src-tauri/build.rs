fn main() {
    // Con el toolchain GNU, WebView2Loader.dll se enlaza dinámicamente. Esta biblioteca de
    // importación diferida (generada con dlltool --output-delaylib) retrasa su carga hasta el
    // primer uso, lo que permite llevar la DLL dentro del ejecutable y extraerla al iniciar.
    // Se copia a OUT_DIR porque Cargo da prioridad a las rutas de búsqueda dentro de target/.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        let lib = "libWebView2Loader.dll.a";
        let src = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("resources").join("delayload").join(lib);
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("delayload");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::copy(&src, out.join(lib)).unwrap();
        println!("cargo:rerun-if-changed={}", src.display());
        println!("cargo:rustc-link-search=native={}", out.display());
    }
    tauri_build::build()
}
