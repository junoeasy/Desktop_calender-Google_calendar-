fn main() {
    println!("cargo:rerun-if-changed=../.env");
    if let Ok(contents) = std::fs::read_to_string("../.env") {
        for line in contents.lines() {
            let Some((key, value)) = line.trim().split_once('=') else { continue };
            let key = key.trim().trim_start_matches('\u{feff}');
            if !matches!(key, "GOOGLE_OAUTH_CLIENT_ID" | "GOOGLE_OAUTH_CLIENT_SECRET") || std::env::var_os(key).is_some() { continue; }
            let value = value.trim().trim_matches(['\'', '"']);
            println!("cargo:rustc-env={key}={value}");
        }
    }
    tauri_build::build()
}
