use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_file_in_globs(patterns: &[&str]) -> Option<PathBuf> {
    for p in patterns {
        let path = Path::new(p);
        if path.exists() {
            return Some(path.to_path_buf());
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=icon.ico");

    #[cfg(target_os = "windows")]
    {
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let icon_path = manifest_dir.join("icon.ico");

        if icon_path.exists() {
            let rc_path = out_dir.join("app.rc");
            let res_path = out_dir.join("app.res");
            let obj_path = out_dir.join("app.obj");

            let rc_content = format!(
                "1 ICON \"{}\"\n",
                icon_path.to_str().unwrap().replace('\\', "/")
            );
            std::fs::write(&rc_path, rc_content).unwrap();

            let rc_exe = find_file_in_globs(&[
                r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\rc.exe",
                r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\rc.exe",
                r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22000.0\x64\rc.exe",
            ]);

            let cvtres_exe = find_file_in_globs(&[
                r"C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools\VC\Tools\MSVC\14.29.30133\bin\Hostx64\x64\cvtres.exe",
                r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.38.33130\bin\Hostx64\x64\cvtres.exe",
            ]);

            if let (Some(rc), Some(cvtres)) = (rc_exe, cvtres_exe) {
                let rc_status = Command::new(&rc)
                    .arg(format!("/fo{}", res_path.display()))
                    .arg(&rc_path)
                    .status();

                if let Ok(st) = rc_status {
                    if st.success() {
                        let cvtres_status = Command::new(&cvtres)
                            .arg("/MACHINE:X64")
                            .arg(format!("/OUT:{}", obj_path.display()))
                            .arg(&res_path)
                            .status();

                        if let Ok(cst) = cvtres_status {
                            if cst.success() {
                                println!("cargo:rustc-link-arg={}", obj_path.display());
                                return;
                            }
                        }
                    }
                }
            }
        }

        // Fallback to winres if custom compiling failed
        let mut res = winres::WindowsResource::new();
        res.set_icon("icon.ico");
        let _ = res.compile();
    }
}
